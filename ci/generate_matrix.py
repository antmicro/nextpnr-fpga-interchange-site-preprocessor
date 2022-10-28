#!/usr/bin/env python3

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

# -- Config --------------------------------------------------------

build_types = [
    'debug',
    'release'
]

devices = [
    'xc7a35t',
    'xczu7ev'
]

# -----------------------------------------------------------------

from argparse import ArgumentParser
import os
import json

def generate_configs():
    configs = []
    for build_type in build_types:
        for device in devices:
            configs.append({
                'build_type': build_type,
                'device': device,
            })
    return configs

def generate_github_matrix():
    configs = generate_configs()
    github_env_file = os.environ['GITHUB_ENV']
    with open(github_env_file, 'a') as f:
        f.write(f'matrix={json.dumps(configs)}')

def generate_gitlab_matrix():
    configs = generate_configs()
    gitlab_matrix_path = os.environ['GITLAB_MATRIX_PATH']
    assert(gitlab_matrix_path is not None)
    with open(gitlab_matrix_path, 'w') as f:
        f.write(json.dumps(configs))

def main():
    parser = ArgumentParser()
    parser.add_argument('mode', choices=['github', 'gitlab'])
    args = parser.parse_args()

    if args.mode == 'github':
        generate_github_matrix()
    elif args.mode == 'gitlab':
        generate_gitlab_matrix()

if __name__ == '__main__':
    main()
