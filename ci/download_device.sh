#!/bin/bash

# Copyright (C) 2022 Antmicro
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0

set -e

DOWNLOAD_DIR=downloads

DEVICE=$1

LATEST_LINK_LINK=https://github.com/chipsalliance/fpga-interchange-tests/releases/download/latest/interchange-${DEVICE}-latest

mkdir $DOWNLOAD_DIR
wget $LATEST_LINK_LINK -O ${DOWNLOAD_DIR}/${DEVICE}-latest_link
wget `cat ${DOWNLOAD_DIR}/${DEVICE}-latest_link` -O ${DOWNLOAD_DIR}/${DEVICE}.tar.xz
tar -xvf downloads/${DEVICE}.tar.xz ${DEVICE}/${DEVICE}.device
rm downloads/${DEVICE}.tar.xz
