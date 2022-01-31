use core::alloc::AllocError;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::aarch64::kernel::{get_boot_info_address, get_limit, get_ram_address};
use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize};
use crate::arch::aarch64::mm::{PhysAddr, VirtAddr};
use crate::environment::is_uhyve;
use crate::mm;
use crate::mm::freelist::{FreeList, FreeListEntry};
use crate::synch::spinlock::SpinlockIrqSave;

static PHYSICAL_FREE_LIST: SpinlockIrqSave<FreeList> = SpinlockIrqSave::new(FreeList::new());
static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

fn detect_from_uhyve() -> Result<(), ()> {
	if !is_uhyve() {
		return Err(());
	}

	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let entry = FreeListEntry {
		start: mm::kernel_end_address().as_usize(),
		end: limit,
	};
	TOTAL_MEMORY.store(
		limit - mm::kernel_end_address().as_usize(),
		Ordering::SeqCst,
	);
	PHYSICAL_FREE_LIST.lock().list.push_back(entry);

	Ok(())
}

fn detect_from_qemu() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let boot_info = align_down!(get_boot_info_address().as_usize(), BasePageSize::SIZE);

	let entry = FreeListEntry {
		start: get_ram_address().as_usize(),
		end: boot_info,
	};
	let mut total: usize = boot_info - get_ram_address().as_usize();
	PHYSICAL_FREE_LIST.lock().list.push_back(entry);

	let entry = FreeListEntry {
		start: boot_info + BasePageSize::SIZE,
		end: mm::kernel_start_address().as_usize() - crate::KERNEL_STACK_SIZE,
	};
	total = mm::kernel_start_address().as_usize()
		- crate::KERNEL_STACK_SIZE
		- boot_info
		- BasePageSize::SIZE;
	PHYSICAL_FREE_LIST.lock().list.push_back(entry);

	let entry = FreeListEntry {
		start: mm::kernel_end_address().as_usize(),
		end: limit,
	};
	total = limit - mm::kernel_end_address().as_usize();
	PHYSICAL_FREE_LIST.lock().list.push_back(entry);

	TOTAL_MEMORY.store(total, Ordering::SeqCst);

	Ok(())
}

pub fn init() {
	detect_from_uhyve()
		.or_else(|_e| detect_from_qemu())
		.expect("Unable to determine physical address space!");
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::SeqCst)
}

pub fn init_page_tables() {}

pub fn allocate(size: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(size, None)?
			.try_into()
			.unwrap(),
	))
}

pub fn allocate_aligned(size: usize, alignment: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert!(alignment > 0);
	assert_eq!(
		size % alignment,
		0,
		"Size {:#X} is not a multiple of the given alignment {:#X}",
		size,
		alignment
	);
	assert_eq!(
		alignment % BasePageSize::SIZE,
		0,
		"Alignment {:#X} is not a multiple of {:#X}",
		alignment,
		BasePageSize::SIZE
	);

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(size, Some(alignment))?
			.try_into()
			.unwrap(),
	))
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: PhysAddr, size: usize) {
	assert!(
		physical_address >= PhysAddr(mm::kernel_end_address().as_u64()),
		"Physical address {:#X} is not >= KERNEL_END_ADDRESS",
		physical_address
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	PHYSICAL_FREE_LIST
		.lock()
		.deallocate(physical_address.as_usize(), size);
}

pub fn print_information() {
	PHYSICAL_FREE_LIST
		.lock()
		.print_information(" PHYSICAL MEMORY FREE LIST ");
}
