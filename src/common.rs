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

pub trait IcStr<'a> {
    fn ic_str(&self, id: u32) -> &'a str;
}

impl<'a> IcStr<'a> for crate::ic_loader::archdef::Root<'a> {
    fn ic_str(&self, id: u32) -> &'a str {
        self.get_str_list().unwrap().get(id).unwrap()
    }
}

/* Splits a range into `slices` possibly even ranges  */
pub fn split_range_nicely(range: std::ops::Range<usize>, slices: usize)
    -> impl Iterator<Item = std::ops::Range<usize>> where
{
    let len = range.end - range.start;
    let split_sz = len / slices;
    let total = split_sz * slices;
    let left = len - total;
    
    (0 .. slices)
        .scan((0, left), move |(current_idx, left), _| {
            let my_len = if *left > 0 {
                *left -= 1;
                split_sz + 1
            } else {
                split_sz
            };
            let range = *current_idx .. (*current_idx + my_len);
            *current_idx += my_len;
            return Some(range);
        })
        .filter(|range| range.start != range.end)
}
