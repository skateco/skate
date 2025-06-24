<div align="center">
  <h1 align="center">Skate</h1>

  <p align="center">Mini-paas for deploying kubernetes manifests.</p>
  <p align="center">Simpler, daemonless & small resource footprint.</p>

Docs -> [skateco.github.io](https://skateco.github.io)
</div>

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![](https://img.shields.io/github/v/release/skateco/skate)](https://github.com/skateco/skate/releases)

- Service discovery - DNS based service discovery.
- Multi-host network - Designed to run on several nodes.
- Kubernetes manifests - Deploy using the same syntax you use in your day job, without the burden of running k8s yourself.
- Small resource footprint - Skate is written in rust, runs no daemon of it’s own and uses minimal resources.
- Https by default - Ingress resources get LetsEncrypt TLS by default.

Born out of the frustration of having to learn yet another deployment configuration file syntax.

Skate runs as a CLI on your machine and talks to a small binary on each host over ssh.

Leverages [podman kube play](https://docs.podman.io/en/latest/markdown/podman-kube-play.1.html) to run pod manifests.

Supported server linux distros: Ubuntu 24.04 (x86_64, aarch64), Fedora 43 (x86_64, aarch64)

Supported client os: macOs (aarch64), Linux (x86_64, aarch64)

You can deploy:

- Pods
- Deployments
- DaemonSets
- CronJobs
- Ingress
- Secrets
- Services

An nginx ingress runs on port 80 and 443 on all nodes.
Lets-encrypt provides the certificates.

## Getting Started

See the [quickstart](https://skateco.github.io/docs/getting-started/) for a guide on how to get started.

Or leeroy jenkins it:

```shell
curl -sL https://raw.githubusercontent.com/skateco/skate/refs/heads/main/hack/install-skate.sh | bash
```

### Built on
- Podman
- Openresty
- Coredns
- LVS
- Keepalived
- Systemd

### Supported manifest attributes

For pods (which affects deployments, daemonsets and cronjobs), see [podman kube play's documentation](https://docs.podman.io/en/latest/markdown/podman-kube-play.1.html#podman-kube-play-support)

For other resources, I'll add some documentation soon.
Check [./hack/](./hack/) for examples of what's been tested.


## Developing

### Mac

Native:

Install the targets:

```shell
rustup target add x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu
````

Install the cross toolchains:

```shell
brew tap messense/macos-cross-toolchains
# install x86_64-unknown-linux-gnu toolchain
brew install x86_64-unknown-linux-gnu
# install aarch64-unknown-linux-gnu toolchain
brew install aarch64-unknown-linux-gnu

#add following to ~/.profile
PATH="${PATH}:/usr/local/Cellar/x86_64-unknown-linux-gnu/13.3.0.reinstall/bin/"
PATH="${PATH}:/usr/local/Cellar/aarch64-unknown-linux-gnu/13.3.0.reinstall/bin/"
# or whatever path brew puts the binaries in

```

```shell
make amd64
## or
make aarch64
```

Or just use [https://github.com/cross-rs/cross](https://github.com/cross-rs/cross)




```shell
make amd64-cross
## or
make aarch64-cross
```

Note: you may need to run this to get cross to work, ([issue](https://github.com/cross-rs/cross/issues/1628)):
```shell
docker run --privileged --rm tonistiigi/binfmt --install amd64
```

### Ubuntu

```shell
# multipass image doesn't have much
sudo apt-get install -y gcc make libssl-dev pkg-config protobuf-compiler
```

