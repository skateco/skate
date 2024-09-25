<div align="center">
  <h1 align="center">Skate</h1>
  <p align="center">Daemonless low footprint self-hosted mini-paas with support for deploying kubernetes manifests.</p>

Docs -> [skateco.github.io](https://skateco.github.io)
</div>


Born out of the frustration of having to learn yet another deployment configuration file syntax.

Skate runs as a CLI on your machine and talks to a small binary on each host over ssh.

Leverages [podman kube play](https://docs.podman.io/en/latest/markdown/podman-kube-play.1.html) to run pod manifests.


Supported Distro: Ubuntu 24.04
Supported architectures: amd64, arm64

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

### Ubuntu

```shell
# multipass image doesn't have much
sudo apt-get install -y gcc make libssl-dev pkg-config
```

