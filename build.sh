#!/bin/bash

podman build -t bots .
podman save bots:latest | gzip > /mnt/docker/bots.tar.gz
