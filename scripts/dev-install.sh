#!/usr/bin/env bash
if [[ -x "${HOME}/.cargo/bin/aichat" ]]; then
  printf "[DEV INSTALLER]: %s\n" "found existing aichat install. moving it aside before compiling"
  mv "${HOME}/.cargo/bin/aichat" "${HOME}/.cargo/bin/aichat_tmp"
  cargo install --path .
  if [[ ! -x "${HOME}/.cargo/bin/aichat_tmp" ]] || [[ ! -x "${HOME}/.cargo/bin/aichat" ]]; then
    printf "[DEV INSTALLER]: %s\n" "could not find the new or temporary production aichat app....exiting"
    exit 3
  else
    # move the dev install to aichat-dev
    mv "${HOME}/.cargo/bin/aichat" "${HOME}/.cargo/bin/aichat-dev"
    # move the tmp production build back
    mv "${HOME}/.cargo/bin/aichat_tmp" "${HOME}/.cargo/bin/aichat"
    printf "[DEV INSTALLER]: %s\n" "aichat-dev installed"
  fi
fi

