/* Copyright (C) 2022 Antmicro
 * 
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 * 
 *     https://www.apache.org/licenses/LICENSE-2.0
 * 
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */


/* Note:
 * This breaks cross-compilation. An alternative trick is to check the `cfg`
 * in `build.rs`, set `cargo:rust-cfg=` based on that and use that here, but this
 * is not recognized by rust-analyzer.
 */

#[cfg(unix)]
macro_rules! include_interchange_capnp {
    ($filename:literal) => {
        include!(concat!(env!("OUT_DIR"), "/interchange/", $filename));
    };
}

#[cfg(windows)]
macro_rules! include_interchange_capnp {
    ($filename:literal) => {
        include!(concat!(env!("OUT_DIR"), "\\interchange\\", $filename));
    };
}
