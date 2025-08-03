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

# Modified for skate

ARG BASE_IMAGE=ubuntu:24.04
FROM $BASE_IMAGE AS base

COPY --chmod=0755 files/usr/local/bin/clean-install /usr/local/bin/
#COPY --chmod=0644 files/etc/systemd/system/* /etc/systemd/system/

RUN apt-get -y update; \
DEBIAN_FRONTEND=noninteractive clean-install \
     podman \
     netavark \
     uidmap \
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
     conntrack iptables nftables iproute2 ethtool util-linux mount kmod \
     libseccomp2 pigz fuse-overlayfs \
     nfs-common open-iscsi \
     bash ca-certificates curl jq procps \
     && find /lib/systemd/system/sysinit.target.wants/ -name "systemd-tmpfiles-setup.service" -delete \
        && rm -f /lib/systemd/system/multi-user.target.wants/* \
        && rm -f /etc/systemd/system/*.wants/* \
        && rm -f /lib/systemd/system/local-fs.target.wants/* \
        && rm -f /lib/systemd/system/sockets.target.wants/*udev* \
        && rm -f /lib/systemd/system/sockets.target.wants/*initctl* \
        && rm -f /lib/systemd/system/basic.target.wants/* \
        && echo "ReadKMsg=no" >> /etc/systemd/journald.conf \
        && ln -s "$(which systemd)" /sbin/init

RUN useradd skate; \
echo skate:200000:65536 > /etc/subuid; \
echo skate:200000:65536 > /etc/subgid;

VOLUME /var/lib/containers
VOLUME /home/skate/.local/share/containers

ADD https://raw.githubusercontent.com/containers/image_build/master/podman/containers.conf /etc/containers/podman-image-containers.conf
ADD https://raw.githubusercontent.com/containers/image_build/master/podman/podman-containers.conf /home/podman/.config/containers/containers.conf
ADD https://raw.githubusercontent.com/containers/storage/main/storage.conf /etc/containers/storage.conf

#[containers]
 #netns="host"
 #userns="host"
 #ipcns="host"
 #utsns="host"
 #cgroupns="host"
 #cgroups="disabled"
 #log_driver = "k8s-file"
 #[engine]
 #cgroup_manager = "cgroupfs"
 #events_logger="file"
 #runtime="crun"

# chmod containers.conf and adjust storage.conf to enable Fuse storage.
RUN mkdir -p /etc/containers && \
    cp /usr/share/containers/containers.conf /etc/containers/ && \
    chmod 644 /etc/containers/containers.conf && \
    echo "server=1.1.1.1" >> /etc/dnsmasq.conf && \
    echo "server=8.8.8.8" >> /etc/dnsmasq.conf && \
    sed -i -e \
        's|^[\#]\?netns\s\?=.*|netns="host"|g' -e \
        's|^[\#]\?userns\s\?=.*|userns="host"|g' -e \
        's|^[\#]\?ipcns\s\?=.*|ipcns="host"|g' -e \
        's|^[\#]\?utsns\s\?=.*|utsns="private"|g' -e \
        's|^[\#]\?cgroupns\s\?=.*|cgroupns="host"|g' -e \
        's|^[\#]\?cgroups\s\?=.*|cgroups="disabled"|g' -e \
        's|^[\#]\?cgroup_manager\s\?=.*|cgroup_manager="cgroupfs"|g' -e \
        's|^[\#]\?events_logger\s\?=.*|events_logger="file"|g' \
        /etc/containers/containers.conf && \
    sed -i -e 's|^#mount_program|mount_program|g' -e \
        '/additionalimage.*/a "/var/lib/shared",' -e \
        's|^mountopt[[:space:]]*=.*$|mountopt = "nodev,fsync=0"|g' \
        /etc/containers/storage.conf && \
    chmod 644 /etc/containers/storage.conf && \
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
