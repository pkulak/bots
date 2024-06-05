#!/usr/bin/env bash

podman build -t docker.io/bots:latest .
podman save docker.io/bots:latest | gzip > /mnt/docker/bots.tar.gz
