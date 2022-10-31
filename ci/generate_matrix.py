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

from argparse import ArgumentParser
import os
import json

def generate_github_matrix(config):
    github_env_file = os.environ['GITHUB_OUTPUT']
    with open(github_env_file, 'a', encoding='utf-8') as f:
        f.write(f'matrix={json.dumps(config)}\n')

def generate_gitlab_matrix(config):
    configs = []
    for build_type in config['build_types']:
        for device in config['devices']:
            configs.append({
                'build_type': build_type,
                'device': device,
            })
    
    gitlab_matrix_path = os.environ['GITLAB_MATRIX_PATH']
    assert(gitlab_matrix_path is not None)
    with open(gitlab_matrix_path, 'w') as f:
        f.write(json.dumps(configs))

def main():
    parser = ArgumentParser()
    parser.add_argument('mode', choices=['github', 'gitlab'])
    parser.add_argument('config_path', type=str)
    args = parser.parse_args()

    with open(args.config_path, 'r') as f:
        config = json.loads(f.read())

    if args.mode == 'github':
        generate_github_matrix(config)
    elif args.mode == 'gitlab':
        generate_gitlab_matrix(config)
    else:
        raise RuntimeError('Incorrect invocation')

if __name__ == '__main__':
    main()
