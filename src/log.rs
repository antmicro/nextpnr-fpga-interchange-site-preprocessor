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


lazy_static! {
    pub static ref DBG_LOG_LEVEL: usize = {
        use std::env;

        match env::var("NISP_DBG_LOG_LEVEL") {
            Ok(lvl) => usize::from_str_radix(&lvl, 10).unwrap(),
            Err(_) => 0,            
        }
    };

    pub static ref DBG_PRINT_CODE_INFO: usize = {
        use std::env;

        match env::var("NISP_PRINT_CODE_INFO") {
            Ok(lvl) => usize::from_str_radix(&lvl, 10).unwrap(),
            Err(_) => 0,            
        }
    };
}

pub const DBG_CRITICAL: usize = 0;
pub const DBG_WARN: usize = 1;
pub const DBG_INFO: usize = 2;
pub const DBG_EXTRA1: usize = 3;
pub const DBG_EXTRA2: usize = 4;

pub const LOG_LVL_STR: &'static [&'static str] = &[
    /* 0 */ "CRITICAL",
    /* 1 */ "WARNING",
    /* 2 */ "INFO",
    /* 3 */ "EXTRA INFO",
    /* 4 */ "EXTRA INFO"
];

#[cfg(debug_assertions)]
macro_rules! dbg_log {
    ($lvl:expr, $fmt:literal $(, $v:expr )*) => {
        let lvl = crate::log::LOG_LVL_STR.len().min($lvl);
        if *crate::log::DBG_LOG_LEVEL >= lvl {
            if *crate::log::DBG_PRINT_CODE_INFO != 0 {
                dbg!(
                    concat!("{}: ", $fmt),
                    $fmt, LOG_LVL_STR[lvl] $(, &$v )*
                );
            } else {
                eprintln!(
                    concat!("{}: ", $fmt),
                    crate::log::LOG_LVL_STR[lvl] $(, &$v )*
                );
            }
        }
    }
}

#[cfg(not(debug_assertions))]
macro_rules! dbg_log {
    ($lvl:expr, $fmt:literal $(, $v:expr )*) => {
        /* NOP */
    };
}