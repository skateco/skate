# Copyright 2018 The Kubernetes Authors.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Modified for skate and fedora

ARG BASE_IMAGE=fedora:43
FROM $BASE_IMAGE AS base

#COPY --chmod=0644 files/etc/systemd/system/* /etc/systemd/system/

RUN sudo check-update; sudo dnf -y update; dnf -y install \
     iproute \
     awk \
     podman \
     netavark \
     # uidmap \
     ipvsadm \
     sudo \
     vim \
     openssh-server \
     systemd \
     dbus \
     dnsmasq \
     wget \
     dnsutils \
     catatonit \
     rsyslog \
     keepalived \
     conntrack iptables nftables \
     #iproute2 \
      ethtool util-linux mount kmod \
     #libseccomp2 \
     pigz \
     # fuse-overlayfs \
     #nfs-common \
     #open-iscsi \
     bash ca-certificates curl jq procps \
     && find /lib/systemd/system/sysinit.target.wants/ -name "systemd-tmpfiles-setup.service" -delete \
        && rm -f /lib/systemd/system/multi-user.target.wants/* \
        && rm -f /etc/systemd/system/*.wants/* \
        && rm -f /lib/systemd/system/local-fs.target.wants/* \
        && rm -f /lib/systemd/system/sockets.target.wants/*udev* \
        && rm -f /lib/systemd/system/sockets.target.wants/*initctl* \
        && rm -f /lib/systemd/system/basic.target.wants/* \
        && echo "ReadKMsg=no" >> /etc/systemd/journald.conf

RUN groupadd skate && useradd skate -g skate; \
echo skate:200000:65536 > /etc/subuid; \
echo skate:200000:65536 > /etc/subgid;

RUN useradd syslog -g adm

VOLUME /var/lib/containers
VOLUME /home/skate/.local/share/containers

ADD https://raw.githubusercontent.com/containers/image_build/master/podman/containers.conf /etc/containers/podman-image-containers.conf
ADD https://raw.githubusercontent.com/containers/image_build/master/podman/podman-containers.conf /home/podman/.config/containers/containers.conf
ADD https://raw.githubusercontent.com/containers/storage/main/storage.conf /etc/containers/storage.conf

COPY <<EOF /root/.config/containers/containers.conf
[containers]
netns="host"
userns="host"
ipcns="host"
utsns="private"
cgroupns="host"
cgroups="disabled"
[engine]
cgroup_manager = "cgroupfs"
events_logger="file"
EOF

# chmod containers.conf and adjust storage.conf to enable Fuse storage.
RUN mkdir -p /etc/containers /etc/skate && \
    echo "server=1.1.1.1" >> /etc/dnsmasq.conf && \
    echo "server=8.8.8.8" >> /etc/dnsmasq.conf && \
    sed -i -e 's|^#mount_program|mount_program|g' -e \
        '/additionalimage.*/a "/var/lib/shared",' -e \
        's|^mountopt[[:space:]]*=.*$|mountopt = "nodev,fsync=0"|g' \
        /etc/containers/storage.conf && \
    chmod 644 /etc/containers/storage.conf && \
    sed -i -e \
        's|^[\#]\?short-name-mode\s\?=.*|short-name-mode="permissive"|g' \
       /etc/containers/registries.conf && \
    mkdir -p /etc/containers/networks && \
    mkdir -p /home/skate/.config/containers &&  \
    mkdir -p /etc/sudoers.d && \
    echo "skate ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/skate && \
    mkdir -p /home/skate/.ssh &&  \
    mkdir -p /home/skate/.local/share/containers/storage &&  \
    chown -R skate:skate /home/skate

RUN mkdir -p /var/lib/shared/overlay-images /var/lib/shared/overlay-layers /var/lib/shared/vfs-images /var/lib/shared/vfs-layers; touch /var/lib/shared/overlay-images/images.lock; touch /var/lib/shared/overlay-layers/layers.lock; touch /var/lib/shared/vfs-images/images.lock; touch /var/lib/shared/vfs-layers/layers.lock

ENV _CONTAINERS_USERNS_CONFIGURED=""

ENV container=docker
# systemd exits on SIGRTMIN+3, not SIGTERM (which re-executes it)
# https://bugzilla.redhat.com/show_bug.cgi?id=1201657
STOPSIGNAL SIGRTMIN+3

COPY --chmod=0755 files/usr/local/bin/ /usr/local/bin/

# NOTE: this is *only* for documentation, the entrypoint is overridden later
ENTRYPOINT [ "/usr/local/bin/entrypoint", "/sbin/init" ]
