//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.

mod address;
mod error;
mod frame_allocator;
mod heap_allocator;
mod memory_area;
mod memory_set;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use address::{StepByOne, VPNRange};
pub use error::{MemoryResult, MemoryError, AreaError, PageError, PagePermissionError};
pub use frame_allocator::{frame_alloc, FrameTracker};
pub use memory_set::{kernel_stack_position, remap_test, MemorySet, KERNEL_SPACE};
pub use memory_area::{MapArea, MapPermission, MapType};
pub use page_table::{translated_byte_buffer, PageTableEntry};
use page_table::{PTEFlags, PageTable};

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
