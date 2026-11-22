#!/bin/sh
# One-time host setup for Debian 13 (ADR-0021). Run as root; safe to run again.
#   ssh root@<host> 'sh -s' < infra/server/bootstrap.sh
# Afterwards: put the deploy key in /home/deploy/.ssh/authorized_keys and create
# /opt/routune/.env (see infra/production/README.md).
set -eu
export DEBIAN_FRONTEND=noninteractive

echo "==> packages"
apt-get update -q
apt-get upgrade -yq
apt-get install -yq ca-certificates curl gnupg ufw unattended-upgrades fail2ban

echo "==> docker (official repository)"
if ! command -v docker > /dev/null; then
	install -m 0755 -d /etc/apt/keyrings
	curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
	chmod a+r /etc/apt/keyrings/docker.asc
	. /etc/os-release
	echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian $VERSION_CODENAME stable" \
		> /etc/apt/sources.list.d/docker.list
	apt-get update -q
	apt-get install -yq docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
fi
# Container logs are capped per app in compose; this is the default for everything else.
if [ ! -f /etc/docker/daemon.json ]; then
	printf '{\n  "log-driver": "json-file",\n  "log-opts": { "max-size": "10m", "max-file": "5" }\n}\n' \
		> /etc/docker/daemon.json
	systemctl restart docker
fi
systemctl enable --now docker

echo "==> deploy user"
id deploy > /dev/null 2>&1 || useradd --create-home --shell /bin/bash deploy
usermod -aG docker deploy
install -d -m 700 -o deploy -g deploy /home/deploy/.ssh
touch /home/deploy/.ssh/authorized_keys
chown deploy:deploy /home/deploy/.ssh/authorized_keys
chmod 600 /home/deploy/.ssh/authorized_keys
install -d -o deploy -g deploy /opt/edge /opt/edge/sites /opt/routune /opt/routune/images /opt/routune/postgres-init

echo "==> firewall (ssh, http, https)"
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw allow 443/udp
ufw --force enable

echo "==> ssh: keys only"
cat > /etc/ssh/sshd_config.d/10-hardening.conf << 'EOF'
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin prohibit-password
EOF
sshd -t && systemctl reload ssh

echo "==> automatic security updates"
printf 'APT::Periodic::Update-Package-Lists "1";\nAPT::Periodic::Unattended-Upgrade "1";\n' \
	> /etc/apt/apt.conf.d/20auto-upgrades
systemctl enable --now unattended-upgrades fail2ban

echo "==> done"
