{ pkgs, lib, config, inputs, ... }:

{
  # https://devenv.sh/basics/
  env.GREET = "devenv";
  env.SSH_PRIVATE_KEY = "/tmp/skate-e2e-key";
  env.SKATELET_DB_PATH = "${config.devenv.root}/skatelet.db";
  env.DATABASE_URL = "sqlite:${config.devenv.root}/skatelet.db";

  # https://devenv.sh/packages/
    packages = [
      pkgs.git
      pkgs.openssl
      pkgs.docker
      pkgs.go
      pkgs.sqlite
    ] ++ lib.optionals pkgs.stdenv.isDarwin [
      # Seems like some part of sqlx needs this if on mac
      # Symptom was "ld: framework not found SystemConfiguration"
      pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
    ];


  # https://devenv.sh/languages/
  # languages.rust.enable = true;

  # https://devenv.sh/processes/
  # processes.cargo-watch.exec = "cargo-watch";

  # https://devenv.sh/services/
  # services.postgres.enable = true;

  # https://devenv.sh/scripts/
  scripts.hello.exec = ''
    echo Welcome to skate
  '';

  enterShell = ''
    hello
  '';

  # https://devenv.sh/tasks/
  # tasks = {
  #   "myproj:setup".exec = "mytool build";
  #   "devenv:enterShell".after = [ "myproj:setup" ];
  # };

  # https://devenv.sh/tests/
  enterTest = ''
    echo "Running tests"
    git --version | grep --color=auto "${pkgs.git.version}"
  '';

  # https://devenv.sh/pre-commit-hooks/
  pre-commit.hooks.shellcheck.enable = true;
  pre-commit.hooks.rustfmt.enable = true;

  scripts = {
    "clippy:run".exec = "cargo clippy --all";
    "clippy:fix".exec = "cargo clippy --fix --all";
  };

  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustchannel
    channel = "stable";
    targets = [] ++ lib.optionals pkgs.stdenv.isDarwin [ "aarch64-unknown-linux-musl" ];
  };

}
