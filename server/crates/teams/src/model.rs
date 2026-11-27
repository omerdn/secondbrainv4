//! Team DTOs (PRD TK1–TK3).

use access::Relation;
use chrono::{DateTime, Utc};
use kernel::{DomainEvent, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Longest team name.
pub const NAME_MAX_CHARS: usize = 80;

/// A role in a team (TK3), strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TeamRole {
    Admin,
    Manager,
    Member,
    Guest,
}

impl TeamRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Manager => "manager",
            Self::Member => "member",
            Self::Guest => "guest",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Admin, Self::Manager, Self::Member, Self::Guest]
            .into_iter()
            .find(|role| role.as_str() == value)
    }

    /// The `access` relation (team → user) of the role.
    #[must_use]
    pub const fn relation(self) -> Relation {
        match self {
            Self::Admin => Relation::TeamAdmin,
            Self::Manager => Relation::TeamManager,
            Self::Member => Relation::TeamMember,
            Self::Guest => Relation::TeamGuest,
        }
    }

    /// The same role as `access` names it.
    #[must_use]
    pub const fn access_role(self) -> access::TeamRole {
        match self {
            Self::Admin => access::TeamRole::Admin,
            Self::Manager => access::TeamRole::Manager,
            Self::Member => access::TeamRole::Member,
            Self::Guest => access::TeamRole::Guest,
        }
    }
}

/// A team (TK1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub logo_url: Option<String>,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    /// The caller's role.
    pub role: TeamRole,
}

/// One member, or someone invited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TeamMember {
    pub user_id: Uuid,
    pub display_name: String,
    pub role: TeamRole,
    /// `false` while the invitation waits (TK2).
    pub active: bool,
}

/// `GET /teams/{id}`: a team its members see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TeamDetail {
    pub team: Team,
    pub members: Vec<TeamMember>,
}

/// An invitation waiting for the caller (TK2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TeamInvitation {
    pub team_id: Uuid,
    pub team_name: String,
    pub role: TeamRole,
    pub invited_by: Option<Uuid>,
    #[ts(type = "string")]
    pub invited_at: DateTime<Utc>,
}

/// `POST /teams` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CreateTeamRequest {
    pub name: String,
    /// `#rrggbb`, lower case.
    pub color: String,
    #[serde(default)]
    #[ts(optional)]
    pub logo_url: Option<String>,
}

fn invalid(field: &str, message: &str) -> FieldError {
    FieldError::new(field, FieldErrorCode::Invalid, message)
}

impl CreateTeamRequest {
    /// Trims and checks the request.
    ///
    /// # Errors
    /// The invalid fields.
    pub fn validate(self) -> Result<Self, ValidationErrors> {
        let name = self.name.trim().to_owned();
        let color = self.color.trim().to_ascii_lowercase();
        let logo_url = self
            .logo_url
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty());
        let mut errors = ValidationErrors::new();
        errors
            .check(kernel::validation::non_blank("name", &name))
            .check(kernel::validation::max_chars("name", &name, NAME_MAX_CHARS));
        let hex = color.len() == 7
            && color.starts_with('#')
            && color[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !hex {
            errors.push(invalid("color", "must be #rrggbb"));
        }
        if let Some(url) = &logo_url
            && (!url.starts_with("https://") || url.chars().count() > 2048)
        {
            errors.push(invalid("logo_url", "must be an https:// URL"));
        }
        errors.into_result().map_err(|err| match err {
            kernel::AppError::Validation(errors) => errors,
            _ => ValidationErrors::new(),
        })?;
        Ok(Self {
            name,
            color,
            logo_url,
        })
    }
}

/// `POST /teams/{id}/invitations` body.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InviteToTeamRequest {
    pub user_id: Uuid,
    pub role: TeamRole,
}

/// `PUT /teams/{id}/members/{user_id}/role` body.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SetRoleRequest {
    pub role: TeamRole,
}

/// Someone was invited to a team (TK2); published in the invitation's transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInvitationCreated {
    pub team_id: Uuid,
    pub from_user: Uuid,
    pub to_user: Uuid,
    pub role: TeamRole,
}

impl DomainEvent for TeamInvitationCreated {
    const EVENT_TYPE: &'static str = "team.invitation_created";
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn tk1_a_team_needs_a_name_and_a_color() {
        let ok = CreateTeamRequest {
            name: "  Ev  ".into(),
            color: "#4F46E5".into(),
            logo_url: Some(" ".into()),
        }
        .validate()
        .unwrap();
        assert_eq!(ok.name, "Ev");
        assert_eq!(ok.color, "#4f46e5");
        assert_eq!(ok.logo_url, None);
        let bad = CreateTeamRequest {
            name: " ".into(),
            color: "blue".into(),
            logo_url: Some("http://x".into()),
        }
        .validate()
        .unwrap_err();
        let fields: Vec<_> = bad.errors().iter().map(|e| e.field.clone()).collect();
        assert_eq!(fields, ["name", "color", "logo_url"]);
    }

    #[test]
    fn roles_round_trip() {
        for role in [
            TeamRole::Admin,
            TeamRole::Manager,
            TeamRole::Member,
            TeamRole::Guest,
        ] {
            assert_eq!(TeamRole::parse(role.as_str()), Some(role));
        }
        assert_eq!(TeamRole::parse("owner"), None);
    }
}
