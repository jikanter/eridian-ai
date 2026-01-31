#!/usr/bin/env bash
#AICHAT_CONFIG_DIR: Overrides the entire configuration directory.
#AICHAT_CONFIG_FILE: Overrides only the config.yaml path.
#AICHAT_ROLES_DIR: Overrides where roles are stored.
#AICHAT_SESSIONS_DIR: Overrides where session history is stored
if [[ ! -x "${HOME}/.cargo/bin/aichat-dev" ]]; then
    printf "[DEV RUN]: %s\n" "aichat-dev binary not found, run dev-install.sh first"
    exit 3
fi

export AICHAT_CONFIG_DIR="/Users/admin/Library/Application Support/aichat-dev"
printf "[DEV RUN]: %s\n" "running aichat-dev with configuration directory ${AICHAT_CONFIG_DIR} "
aichat-dev
