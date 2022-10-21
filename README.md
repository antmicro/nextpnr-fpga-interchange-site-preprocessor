# NISP - Nextpnr-fpga_Interchange Site Preprocessor

## What's NISP

The tool is a simple pin-to-pin site router which gathers information about routability
between pairs of pins within site. The most basic information is whether a possible
route between two given pins exists, but NISP can also gather constraints erquired for the
routes and can account for alternative routes between pins.

The goal of this tool is to create a sort-of cache with routability data for a given
fpga-interchange device, which can be then used to improve performace of
**nextpnr-fpga_interchange**'s SA placer, which currently suffers from a big performance
hit due to long routability checks that happen during cell placement.

Currently the tool is at an early development stage and does not output BBA/binary, but
it can output JSONs with routability information.

## Features

NISP has the following features at the  moment

* Generate site-ruting graph and export it into graphviz .dot files
  (`--dot`, `--dot-prefix` options)
* Generate routability lookup and constraints and export it into JSON
  (`--json`, `--json-prefix` options)
* Optimize constraint formulas (use `--no-formula-opt` to skip that step)

## Building NISP

### Prerequisites

1. Install [Rust](https://www.rust-lang.org/)
2. Clone [fpga-interchange-schema](https://github.com/chipsalliance/fpga-interchange-schema)

### Compiling

1. Set `FPGA_INTERCHANGE_SCHEMA_DIR` environmental variable to point to directory where you
   cloned
   [fpga-interchange-schema](https://github.com/chipsalliance/fpga-interchange-schema).
2. Run `cargo build` for debug build, `cargo build --release` for release build.

## Running NISP

```
nisp [OPTIONS] <DEVICE> <BBA>
```

* `<DEVICE>` - Path to fpga-interchaneg device file
* `<BBA>` - BBA output path. Currently ignored.

Descriptions for currently available options are available when running the program with
`--help` flag.

If an option requires you to specify a list of tiles, you can use `:all` as a replacement
for listing all tiles in the architecture.

### `test` script
This script can be used to simplify compiling, running and debugging NISP.
It's short, so the best way to understand what it does is just tot read it.


-------------------------------------------------

Copyright (c) Antmicro 2022
