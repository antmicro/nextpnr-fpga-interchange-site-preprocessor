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

//! # NISP - Nextpnr-Fpga_Interchange-SitePreprocessor
//! 
//! a simple pin-to-pin site router which gathers information about routability
//! between pairs of pins within site. The most basic information is whether a possible
//! route between two given pins exists, but NISP can also gather constraints required for
//! the routes and can account for alternative routes between pins.
//! 
//! ## Modules
//! 
//! * `log` - logging utilities
//! * `common` - various uncategorized utility functions
//! * `strings` - Handling of string identifiers
//! * `ic_loader` - Loading and exploring fpga-interchange data
//! * `logic_formula` - Utilities for handling and preocessing boolean logic formulas
//! * `router` - Routing of FPGA resources
//! * `exporter` - Exporting data into files and serialization
//! * `dot_exporter` - Writing graphviz _.dot_ files
//! 
//! ## Common nomenclature / Glossary
//! 
//! NISP operates on FPGA-Interchange data. The reader is expected to be familiar with the
//! terms used for its representation of FPGA architectures.
//! Documentation of the format is available
//! [here](https://fpga-interchange-schema.readthedocs.io/).
//! 
//! The nomenclature used in the format also shares a lot of the similarities with
//! AMD/Xilinx's terminology
//! [described in RapidWright docs](https://www.rapidwright.io/docs/Xilinx_Architecture.html).
//! 
//! Plenty of comonnly used names are shortened according to the following scheme:
//! ### FPGA resources:
//! * `tt` - TileType
//! * `st` - SiteType
//! * `stitt` - SiteType in TileType
//!   (TileTypes can hold multiple instances of a one site-type)
//! * `sw` - SiteWire
//! 
//! ### Abstract concepts:
//! * `belpin` - refers to an integer identifier of a pin of a BEL. The identifier is unique
//!   for a given TileType.
//! * `source`/`driver` - a BEL pin driving a net. A net can have only one driver at a time.
//! * `sink` - a BEL pin at the receiving end of a net. A net can have multiple sinks.
//! 

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde;

#[macro_use]
pub mod include_path;
#[macro_use]
pub mod log;
pub mod common;
pub mod strings;
pub mod ic_loader;
pub mod logic_formula;
pub mod router;
pub mod exporter;
pub mod dot_exporter;
