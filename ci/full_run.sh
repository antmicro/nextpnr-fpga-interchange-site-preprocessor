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

if [[ -z "$NISP_PATH" ]]; then
    NISP_PATH=./nisp
fi

DOTS_DIR=dots
JSONS_DIR=jsons
THREADS=`nproc`
COMMON_PREPROCESS_OPTIONS="--threads $THREADS --json-prefix $JSONS_DIR --json :all --dot-prefix $DOTS_DIR --dot :all -c"
DEBUG_GLOBAL_OPTIONS="--raw"
RELEASE_GLOBAL_OPTIONS=""
BUILD_TYPE=$1
DEVICE=$2

mkdir -p $DOTS_DIR
mkdir -p $JSONS_DIR

export RUST_BACKTRACE=1

if [[ "$1" = "debug" ]]; then
    $NISP_PATH ${DEVICE}/${DEVICE}.device.raw ${DEVICE}_nisp.bba $DEBUG_GLOBAL_OPTIONS preprocess $COMMON_PREPROCESS_OPTIONS
elif [[ "$1" = "release" ]]; then
    $NISP_PATH ${DEVICE}/${DEVICE}.device ${DEVICE}_nisp.bba $RELEASE_GLOBAL_OPTIONS preprocess $COMMON_PREPROCESS_OPTIONS
else
    echo "Invalid build type!"
    exit -1
fi
