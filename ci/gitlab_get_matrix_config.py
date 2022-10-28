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

def main():
    parser = ArgumentParser()
    parser.add_argument('ci_node_index', type=int)
    parser.add_argument('variable', type=str)

    args = parser.parse_args()

    gitlab_matrix_path = os.environ['GITLAB_MATRIX_PATH']
    
    with open(gitlab_matrix_path, 'r') as f:
        matrix = json.loads(f.read())
    
    print(matrix[args.ci_node_index - 1][args.variable])

if __name__ == '__main__':
    main()
