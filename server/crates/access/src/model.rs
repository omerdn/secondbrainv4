//! Relation tuples and permission bits (ARCHITECTURE §6.1, §6.2, T1.1.1).

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign};

use uuid::Uuid;

/// Kind of object a relation is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectType {
    Item,
    Team,
}

/// Kind of subject a relation points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubjectType {
    User,
    Team,
    Item,
}

macro_rules! string_enum {
    ($ty:ty { $($variant:ident => $s:literal),+ $(,)? }) => {
        impl $ty {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The value stored in the database.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }

            /// Parses a database value.
            #[must_use]
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$variant),)+ _ => None }
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum!(ObjectType { Item => "item", Team => "team" });
string_enum!(SubjectType { User => "user", Team => "team", Item => "item" });

/// A relation between an object and a subject. Every relation is valid between exactly one
/// object type and one subject type ([`Relation::object_type`], [`Relation::subject_type`]);
/// the database enforces the same shapes (`access_relations_shape_check`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    /// `item → user`: personal owner.
    Owner,
    /// `item → team`: team-owned item.
    OwnedBy,
    /// `item → user`: assignee of an item.
    Assignee,
    /// `item → user`: watcher of an item.
    Watcher,
    /// `team → user`: team roles.
    TeamAdmin,
    TeamManager,
    TeamMember,
    TeamGuest,
    /// `item → item`: page belongs to a notebook.
    InNotebook,
    /// `item → user`: notebook member who can edit its pages.
    Editor,
    /// `item → user`: notebook member who can view its pages.
    Viewer,
    /// `item → item`: page is linked to an item (V3).
    LinkedTo,
}

string_enum!(Relation {
    Owner => "owner",
    OwnedBy => "owned_by",
    Assignee => "assignee",
    Watcher => "watcher",
    TeamAdmin => "admin",
    TeamManager => "manager",
    TeamMember => "member",
    TeamGuest => "guest",
    InNotebook => "in_notebook",
    Editor => "editor",
    Viewer => "viewer",
    LinkedTo => "linked_to",
});

impl Relation {
    /// The object type this relation is attached to.
    #[must_use]
    pub const fn object_type(self) -> ObjectType {
        match self {
            Self::TeamAdmin | Self::TeamManager | Self::TeamMember | Self::TeamGuest => {
                ObjectType::Team
            }
            _ => ObjectType::Item,
        }
    }

    /// The subject type this relation points at.
    #[must_use]
    pub const fn subject_type(self) -> SubjectType {
        match self {
            Self::OwnedBy => SubjectType::Team,
            Self::InNotebook | Self::LinkedTo => SubjectType::Item,
            _ => SubjectType::User,
        }
    }
}

/// One row of `access_relations`: `object relation subject`. Construct it with
/// [`RelationTuple::new`] so the object and subject types always match the relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelationTuple {
    object_id: Uuid,
    relation: Relation,
    subject_id: Uuid,
}

impl RelationTuple {
    /// `object_id` and `subject_id` are interpreted with the relation's object and subject types.
    #[must_use]
    pub const fn new(object_id: Uuid, relation: Relation, subject_id: Uuid) -> Self {
        Self {
            object_id,
            relation,
            subject_id,
        }
    }

    #[must_use]
    pub const fn object_type(&self) -> ObjectType {
        self.relation.object_type()
    }

    #[must_use]
    pub const fn object_id(&self) -> Uuid {
        self.object_id
    }

    #[must_use]
    pub const fn relation(&self) -> Relation {
        self.relation
    }

    #[must_use]
    pub const fn subject_type(&self) -> SubjectType {
        self.relation.subject_type()
    }

    #[must_use]
    pub const fn subject_id(&self) -> Uuid {
        self.subject_id
    }
}

impl fmt::Display for RelationTuple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} {} {}:{}",
            self.object_type(),
            self.object_id,
            self.relation,
            self.subject_type(),
            self.subject_id
        )
    }
}

/// Permission bitmask stored in `item_access.perms` (§6.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Perms(u8);

impl Perms {
    pub const NONE: Self = Self(0);
    pub const VIEW: Self = Self(1 << 0);
    pub const EDIT: Self = Self(1 << 1);
    /// Share the item with others. Which sources grant SHARE, LINK and MANAGE is decided by
    /// `compute_access` (§6.3, T1.1.2).
    pub const SHARE: Self = Self(1 << 2);
    pub const LINK: Self = Self(1 << 3);
    pub const MANAGE: Self = Self(1 << 4);
    pub const DELETE: Self = Self(1 << 5);
    pub const ALL: Self = Self(0b11_1111);

    const NAMES: [(Self, &'static str); 6] = [
        (Self::VIEW, "VIEW"),
        (Self::EDIT, "EDIT"),
        (Self::SHARE, "SHARE"),
        (Self::LINK, "LINK"),
        (Self::MANAGE, "MANAGE"),
        (Self::DELETE, "DELETE"),
    ];

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// `None` if `bits` has bits outside [`Perms::ALL`].
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// `self | other`, usable in constants.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for Perms {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Perms {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Perms {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl fmt::Debug for Perms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("Perms(NONE)");
        }
        let names: Vec<&str> = Self::NAMES
            .iter()
            .filter(|(p, _)| self.contains(*p))
            .map(|(_, n)| *n)
            .collect();
        write!(f, "Perms({})", names.join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn string_enums_round_trip() {
        for r in Relation::ALL {
            assert_eq!(Relation::parse(r.as_str()), Some(*r));
        }
        for t in ObjectType::ALL {
            assert_eq!(ObjectType::parse(t.as_str()), Some(*t));
        }
        for t in SubjectType::ALL {
            assert_eq!(SubjectType::parse(t.as_str()), Some(*t));
        }
        assert_eq!(Relation::parse("nope"), None);
        assert_eq!(ObjectType::parse("user"), None);
        assert_eq!(SubjectType::parse(""), None);
    }

    #[test]
    fn relation_shapes() {
        use Relation::*;
        assert_eq!(OwnedBy.subject_type(), SubjectType::Team);
        assert_eq!(LinkedTo.subject_type(), SubjectType::Item);
        assert_eq!(InNotebook.subject_type(), SubjectType::Item);
        for r in [TeamAdmin, TeamManager, TeamMember, TeamGuest] {
            assert_eq!(r.object_type(), ObjectType::Team);
            assert_eq!(r.subject_type(), SubjectType::User);
        }
        for r in [Owner, Assignee, Watcher, Editor, Viewer] {
            assert_eq!(r.object_type(), ObjectType::Item);
            assert_eq!(r.subject_type(), SubjectType::User);
        }
    }

    #[test]
    fn tuple_display() {
        let t = RelationTuple::new(Uuid::nil(), Relation::OwnedBy, Uuid::max());
        assert_eq!(
            t.to_string(),
            "item:00000000-0000-0000-0000-000000000000 owned_by \
             team:ffffffff-ffff-ffff-ffff-ffffffffffff"
        );
    }

    #[test]
    fn perms_debug() {
        assert_eq!(format!("{:?}", Perms::NONE), "Perms(NONE)");
        assert_eq!(
            format!("{:?}", Perms::VIEW | Perms::EDIT),
            "Perms(VIEW | EDIT)"
        );
    }

    #[test]
    fn perms_bits_are_distinct_and_fill_all() {
        let mut acc = Perms::NONE;
        for (p, _) in Perms::NAMES {
            assert_eq!(p.bits().count_ones(), 1);
            assert!(!acc.contains(p));
            acc |= p;
        }
        assert_eq!(acc, Perms::ALL);
    }

    proptest! {
        #[test]
        fn from_bits_accepts_exactly_valid_masks(bits in any::<u8>()) {
            let parsed = Perms::from_bits(bits);
            prop_assert_eq!(parsed.is_some(), bits < 64);
            if let Some(p) = parsed {
                prop_assert_eq!(p.bits(), bits);
                prop_assert!(Perms::ALL.contains(p));
                prop_assert_eq!(p & Perms::ALL, p);
            }
        }
    }
}
