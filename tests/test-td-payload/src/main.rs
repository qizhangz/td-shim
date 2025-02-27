// Copyright (c) 2021 Intel Corporation
//
// SPDX-License-Identifier: BSD-2-Clause-Patent

#![no_std]
#![no_main]
#![allow(unused)]
#![feature(alloc_error_handler)]
#[macro_use]

mod lib;
mod testacpi;
mod testiorw32;
mod testiorw8;
mod testmemmap;
mod testtdinfo;
mod testtdreport;
mod testtdve;
mod testtrustedboot;

extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::size_of;
use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;
use td_layout::memslice;

use crate::lib::{TestResult, TestSuite};
use crate::testacpi::{TdAcpi, TestTdAcpi};
use crate::testiorw32::Tdiorw32;
use crate::testiorw8::Tdiorw8;
use crate::testmemmap::MemoryMap;
use crate::testtdinfo::Tdinfo;
use crate::testtdreport::Tdreport;
use crate::testtdve::TdVE;
use crate::testtrustedboot::{TdTrustedBoot, TestTdTrustedBoot};

use r_efi::efi::Guid;
use serde::{Deserialize, Serialize};
use serde_json::{Result, Value};
use td_payload::{serial::serial, serial_print};
use td_shim::e820::{E820Entry, E820Type};
use td_shim::{TD_ACPI_TABLE_HOB_GUID, TD_E820_TABLE_HOB_GUID};
use td_uefi_pi::{fv, hob, pi};
use zerocopy::FromBytes;

const E820_TABLE_SIZE: usize = 128;
const PAYLOAD_HEAP_SIZE: usize = 0x100_0000;

#[derive(Debug, Serialize, Deserialize)]
// The test cases' data structure corresponds to the test config json data structure
pub struct TestCases {
    pub tcs001: Tdinfo,
    pub tcs002: Tdinfo,
    pub tcs003: Tdinfo,
    pub tcs004: Tdinfo,
    pub tcs005: Tdinfo,
    pub tcs006: Tdreport,
    pub tcs007: Tdiorw8,
    pub tcs008: Tdiorw32,
    pub tcs009: TdVE,
    pub tcs010: TdAcpi,
    pub tcs011: MemoryMap,
    pub tcs012: MemoryMap,
    pub tcs013: MemoryMap,
    pub tcs014: MemoryMap,
    pub tcs015: MemoryMap,
    pub tcs016: TdTrustedBoot,
}

pub const CFV_FFS_HEADER_TEST_CONFIG_GUID: Guid = Guid::from_fields(
    0xf10e684e,
    0x3abd,
    0x20e4,
    0x59,
    0x32,
    &[0x8f, 0x97, 0x3c, 0x35, 0x5e, 0x57],
); // {F10E684E-3ABD-20E4-5932-8F973C355E57}

#[cfg(not(test))]
#[panic_handler]
#[allow(clippy::empty_loop)]
fn panic(_info: &PanicInfo) -> ! {
    log::info!("panic ... {:?}\n", _info);
    loop {}
}

#[alloc_error_handler]
#[allow(clippy::empty_loop)]
fn alloc_error(_info: core::alloc::Layout) -> ! {
    log::info!("alloc_error ... {:?}\n", _info);
    panic!("deadloop");
}

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[cfg(not(test))]
fn init_heap(heap_start: usize, heap_size: usize) {
    unsafe {
        ALLOCATOR.lock().init(heap_start as *mut u8, heap_size);
    }
}

#[cfg(not(test))]
fn build_testcases() -> TestCases {
    log::info!("Starting get test data from cfv and parse json data\n");
    let cfv = memslice::get_mem_slice(memslice::SliceType::Config);
    let json_data = fv::get_file_from_fv(
        cfv,
        pi::fv::FV_FILETYPE_RAW,
        CFV_FFS_HEADER_TEST_CONFIG_GUID,
    )
    .unwrap();
    let json_string = String::from_utf8_lossy(json_data).to_string();
    // trim zero in json string
    let json_config = json_string.trim_matches(char::from(0));

    serde_json::from_str(json_config).unwrap()
}

#[cfg(not(test))]
#[no_mangle]
#[cfg_attr(target_os = "uefi", export_name = "efi_main")]
extern "C" fn _start(hob: *const c_void) -> ! {
    use td_layout::runtime::*;
    use testmemmap::TestMemoryMap;

    td_logger::init();
    log::info!("Starting rust-tdcall-payload hob - {:p}\n", hob);

    // init heap so that we can allocate memory
    let hob_list = hob::check_hob_integrity(unsafe {
        memslice::get_dynamic_mem_slice_mut(memslice::SliceType::PayloadHob, hob as usize)
    })
    .expect("Integrity check failed: invalid HOB list");

    // There is no heap at this moment, put the E820 table on the stack
    let mut memory_map = [E820Entry::default(); E820_TABLE_SIZE];
    get_memory_map(hob_list, &mut memory_map);

    let heap_base = find_heap_memory(&memory_map).expect("Cannot find memory for heap");
    log::info!("Init heap: {:X} - {:X}\n", heap_base, PAYLOAD_HEAP_SIZE);
    init_heap(heap_base, PAYLOAD_HEAP_SIZE);

    // create TestSuite to hold the test cases
    let mut ts = TestSuite {
        testsuite: Vec::new(),
        passed_cases: 0,
        failed_cases: 0,
    };

    // build test cases with test configuration data in CFV
    let mut tcs = build_testcases();

    // Add test cases in ts.testsuite
    if tcs.tcs001.run {
        ts.testsuite.push(Box::new(tcs.tcs001));
    }

    if tcs.tcs002.run {
        ts.testsuite.push(Box::new(tcs.tcs002));
    }

    if tcs.tcs003.run {
        ts.testsuite.push(Box::new(tcs.tcs003));
    }

    if tcs.tcs004.run {
        ts.testsuite.push(Box::new(tcs.tcs004));
    }

    if tcs.tcs005.run {
        ts.testsuite.push(Box::new(tcs.tcs005));
    }

    if tcs.tcs006.run {
        ts.testsuite.push(Box::new(tcs.tcs006));
    }

    if tcs.tcs007.run {
        ts.testsuite.push(Box::new(tcs.tcs007));
    }

    if tcs.tcs008.run {
        ts.testsuite.push(Box::new(tcs.tcs008));
    }

    if tcs.tcs009.run {
        ts.testsuite.push(Box::new(tcs.tcs009));
    }

    if tcs.tcs010.run && tcs.tcs010.expected.num > 0 {
        let test_acpi = TestTdAcpi {
            hob_address: hob as usize,
            td_acpi: tcs.tcs010,
        };
        ts.testsuite.push(Box::new(test_acpi));
    }

    if tcs.tcs011.run {
        let test_memory_map = TestMemoryMap {
            hob_address: hob as usize,
            case: tcs.tcs011,
        };
        ts.testsuite.push(Box::new(test_memory_map));
    }

    if tcs.tcs012.run {
        let test_memory_map = TestMemoryMap {
            hob_address: hob as usize,
            case: tcs.tcs012,
        };
        ts.testsuite.push(Box::new(test_memory_map));
    }

    if tcs.tcs013.run {
        let test_memory_map = TestMemoryMap {
            hob_address: hob as usize,
            case: tcs.tcs013,
        };
        ts.testsuite.push(Box::new(test_memory_map));
    }

    if tcs.tcs014.run {
        let test_memory_map = TestMemoryMap {
            hob_address: hob as usize,
            case: tcs.tcs014,
        };
        ts.testsuite.push(Box::new(test_memory_map));
    }

    if tcs.tcs015.run {
        let test_memory_map = TestMemoryMap {
            hob_address: hob as usize,
            case: tcs.tcs015,
        };
        ts.testsuite.push(Box::new(test_memory_map));
    }

    if tcs.tcs016.run {
        let test_tboot = TestTdTrustedBoot {
            hob_address: hob as usize,
            case: tcs.tcs016,
        };
        ts.testsuite.push(Box::new(test_tboot));
    }

    // run the TestSuite which contains the test cases
    serial_print!("---------------------------------------------\n");
    serial_print!("Start to run tests.\n");
    serial_print!("---------------------------------------------\n");
    ts.run();
    serial_print!(
        "Test Result: Total run {0} tests; {1} passed; {2} failed\n",
        ts.testsuite.len(),
        ts.passed_cases,
        ts.failed_cases
    );

    panic!("deadloop");
}

fn get_memory_map(hob_list: &[u8], e820: &mut [E820Entry]) {
    if let Some(hob) = hob::get_next_extension_guid_hob(hob_list, TD_E820_TABLE_HOB_GUID.as_bytes())
    {
        let table = hob::get_guid_data(hob).expect("Failed to get data from E820 GUID HOB");
        let entry_num = table.len() / size_of::<E820Entry>();
        if entry_num > E820_TABLE_SIZE {
            panic!("Invalid E820 table size");
        }

        let mut offset = 0;
        let mut idx = 0;
        while idx < entry_num {
            if let Some(entry) =
                E820Entry::read_from(&table[offset..offset + size_of::<E820Entry>()])
            {
                // Ignore the padding zero in GUIDed HOB
                if idx == entry_num - 1 && entry == E820Entry::default() {
                    return;
                }
                // save it to table
                e820[idx] = entry;
                idx += 1;
                offset += size_of::<E820Entry>();
            } else {
                panic!("Error parsing E820 table\n");
            }
        }
    } else {
        panic!("There's no E820 table can be found in Payload HOB\n");
    }
}

fn find_heap_memory(memory_map: &[E820Entry]) -> Option<usize> {
    let mut target = None;
    // Find the highest usable memory for heap
    for entry in memory_map {
        if entry.r#type == E820Type::Memory as u32 && entry.size >= PAYLOAD_HEAP_SIZE as u64 {
            target = Some(entry);
        }
    }
    if let Some(entry) = target {
        return Some((entry.addr + entry.size) as usize - PAYLOAD_HEAP_SIZE);
    }
    None
}
