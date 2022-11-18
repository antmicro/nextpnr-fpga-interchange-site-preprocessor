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


use std::path::{Path, PathBuf};
use std::fs::File;
use std::collections::HashSet;
use std::io::Write;
use std::collections::HashMap;

use serde::Serialize;

pub trait AsBytes {
    fn as_bytes<'s>(&'s self) -> &'s [u8];
}

impl AsBytes for String {
    fn as_bytes<'s>(&'s self) -> &'s [u8] {
        String::as_bytes(self)
    }
}

impl AsBytes for str {
    fn as_bytes<'s>(&'s self) -> &'s [u8] {
        str::as_bytes(self)
    }
}

impl AsBytes for [u8] {
    fn as_bytes<'s>(&'s self) -> &'s [u8] {
        self
    }
}

#[derive(Default)]
struct ExportChecker {
    export: HashSet<String>,
    export_all: bool,
}

impl ExportChecker {
    fn should_export(&self, name: &str) -> bool {
        if self.export_all || self.export.contains(name) {
            return true;
        }
        false
    }
}

pub trait Exporter<D> {
    fn ignore_or_export<'s, F>(&'s mut self, name: &str, exporter: F)
        -> std::io::Result<()>
    where
        F: FnOnce() -> D + 's;
    
    fn flush(&mut self) -> std::io::Result<()>;
}

pub struct MultiFileExporter {
    prefix: String,
    suffix: String,
    checker: ExportChecker,
}

impl MultiFileExporter {
    pub fn new(arg_list: &Option<Vec<String>>, prefix: String, suffix: String) -> Self {
        let mut export_all = false;
        let mut export = HashSet::new();
        if let Some(args) = arg_list {
            for arg in args {
                if arg == ":all" {
                    export_all = true;
                } else {
                    export.insert(arg.clone());
                }
            }
        }

        Self { prefix, suffix, checker: ExportChecker { export, export_all } }
    }
}

impl<D> Exporter<D> for MultiFileExporter where D: AsBytes {
    fn ignore_or_export<'s, F>(&'s mut self, name: &str, exporter: F)
        -> std::io::Result<()>
    where
        F: FnOnce() -> D + 's
    {
        if self.checker.should_export(name) {
            let data = exporter();
            let path = Path::new(&self.prefix)
                .join(Path::new(&(name.to_string() + &self.suffix)));
            let mut file = File::create(path)?;
            return file.write(data.as_bytes()).map(|_| ());
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct CompoundJsonExporter<D> where D: Serialize {
    filename: PathBuf,
    data: HashMap<String, D>,
    checker: ExportChecker,
}

impl<D> CompoundJsonExporter<D> where D: Serialize {
    pub fn new(arg_list: &Option<Vec<String>>, filename: PathBuf) -> Self {
        let mut export_all = false;
        let mut export = HashSet::new();
        if let Some(args) = arg_list {
            for arg in args {
                if arg == ":all" {
                    export_all = true;
                } else {
                    export.insert(arg.clone());
                }
            }
        }

        Self {
            filename,
            data: HashMap::new(),
            checker: ExportChecker { export, export_all }
        }
    }
}

impl<D> Exporter<D> for CompoundJsonExporter<D> where D: Serialize {
    fn ignore_or_export<'s, F>(&'s mut self, name: &str, exporter: F)
        -> std::io::Result<()>
    where
        F: FnOnce() -> D + 's
    {
        if self.checker.should_export(name) {
            let data = exporter();
            self.data.insert(name.into(), data);
        }
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let data = serde_json::to_string_pretty(&self.data).unwrap();
        let mut file = File::create(&self.filename)?;
        return file.write(data.as_bytes()).map(|_| ());
    }
}
