//! Implementation of [`MapArea`] and [`MemorySet`].

use super::{MapArea, MapPermission, MapType};
use super::{PTEFlags, PageTable, PageTableEntry};
use super::{PhysAddr, VirtAddr, VirtPageNum};
use super::VPNRange;
use super::error::{AreaError, MemoryResult};
use crate::config::{
    KERNEL_STACK_SIZE, MEMORY_END, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE,
};
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use lazy_static::*;
use riscv::register::satp;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// The kernel's initial memory mapping(kernel address space)
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> =
        Arc::new(unsafe { UPSafeCell::new(MemorySet::new_kernel()) });
}
/// address space
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}

impl MemorySet {
    /// Create a new empty `MemorySet`.
    pub fn new_bare() -> MemoryResult<Self> {
        let pt = PageTable::new()?;
        Ok(Self {
            page_table: pt,
            areas: Vec::new(),
        })
    }
    /// Get the page table token
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// Change: 防止插入冲突
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) -> MemoryResult<()> {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        )
    }
    /// 延迟插入
    pub fn insert_framed_area_lazy(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) -> MemoryResult<()> {
        self.push_lazy(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        )
    }
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) -> MemoryResult<()> {
        map_area.map(&mut self.page_table)?;
        map_area.check_all_page(&mut self.page_table)?; // force allocation
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data)?;
        }
        self.areas.push(map_area);
        Ok(())
    }
    fn push_lazy(&mut self, mut map_area: MapArea, data: Option<&[u8]>) -> MemoryResult<()> {
        map_area.map(&mut self.page_table)?;
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data)?;
        }
        self.areas.push(map_area);
        Ok(())
    }
    /// Mention that trampoline is not collected by areas.
    fn map_trampoline(&mut self) -> MemoryResult<()> {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        )
    }
    /// Without kernel stacks.
    pub fn new_kernel() -> Self {
        let memory_set = Self::new_bare();
        assert!(memory_set.is_ok(), "failed to allocate kernel memory, err = {}", memory_set.err().unwrap());
        let mut memory_set = memory_set.unwrap();
        // map trampoline
        memory_set.map_trampoline().unwrap();
        // map kernel sections
        info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        info!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        info!("Map .text section");
        memory_set.push_lazy(
            MapArea::new(
                (stext as usize).into(),
                (etext as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            None,
        ).unwrap();
        info!("Map .rodata section");
        memory_set.push_lazy(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        ).unwrap();
        info!("Map .data section");
        memory_set.push_lazy(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        ).unwrap();
        info!("Map .bss section");
        memory_set.push_lazy(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        ).unwrap();
        info!("Map physical memory");
        memory_set.push_lazy(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        ).unwrap();
        memory_set
    }
    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp_base and entry point.
    pub fn from_elf(elf_data: &[u8]) -> MemoryResult<(Self, usize, usize)> {
        let mut memory_set = Self::new_bare()?;
        // map trampoline
        memory_set.map_trampoline()?;
        // map program headers of elf, with U flag
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.get_vpn_range().get_end();
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                )?;
            }
        }
        // map user stack with U flags
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into();
        // guard page
        user_stack_bottom += PAGE_SIZE;
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE;
        memory_set.push_lazy(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        )?;
        // used in sbrk
        memory_set.push_lazy(
            MapArea::new(
                user_stack_top.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        )?;
        // map TrapContext
        // 必须严格分配内存以便 trap_handler 工作
        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT_BASE.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W,
            ),
            None,
        )?;
        Ok((
            memory_set,
            user_stack_top,
            elf.header.pt2.entry_point() as usize,
        ))
    }
    /// Change page table by writing satp CSR Register.
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            asm!("sfence.vma");
        }
    }
    /// 将虚拟页号映射到页表
    pub fn transform(&mut self, vpn: VirtPageNum) -> MemoryResult<PageTableEntry> {
        if let Some(area) = self.areas.iter_mut().find(|x|x.get_vpn_range().is_contains(&vpn)) {
            area.check_range(&mut self.page_table, VPNRange::new_by_len(vpn, 1))?;
        } else {
            return Err(AreaError::NotInclude.into())
        };
        self.page_table.translate(vpn)
    }
    /// shrink the area to new_end
    #[allow(unused)]
    pub fn shrink_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> MemoryResult<()> {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.get_vpn_range().get_start() == start.floor())
        {
            area.narrow(&mut self.page_table, new_end.ceil())
        } else {
            Err(AreaError::NotMatch.into())
        }
    }

    /// append the area to new_end
    #[allow(unused)]
    pub fn append_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> MemoryResult<()> {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.get_vpn_range().get_start() == start.floor())
        {
            area.expand(&mut self.page_table, new_end.ceil())
        } else {
            Err(AreaError::NotMatch.into())
        }
    }

    fn is_mapped(&self, range: VPNRange) -> bool {
        self.areas.iter().any(|x|x.get_vpn_range().intersects(&range))
    }

    fn is_unmapped(&self, range: VPNRange) -> bool {
        let count = self.areas.iter().map(|x|{
            let (_, _, rem) = x.get_vpn_range().exclude(&range);
            rem.into_iter().count()
        }).sum::<usize>();

        let expected = range.into_iter().count();
        count != expected
    }

    fn is_critical(&self, vpn: VirtPageNum) -> bool {
        if vpn == VirtPageNum::from(VirtAddr::from(TRAMPOLINE)) {
            return true;
        } else if vpn == VirtPageNum::from(VirtAddr::from(TRAP_CONTEXT_BASE)) {
            return true;
        }
        return false;
    }

    /// 尝试映射虚拟内存
    pub fn map_memory(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) -> MemoryResult<()>  {
        let area = MapArea::new(start_va, end_va, MapType::Framed, permission);
        if area.get_vpn_range().into_iter().any(|x|self.is_critical(x)) {
            return Err(AreaError::CriticalArea.into());
        }
        if self.is_mapped(area.get_vpn_range()) {
            return Err(AreaError::ContainMapped.into());
        }
        self.push_lazy(
            area,
            None,
        )
    }

    /// 尝试取消映射除关键内存之外的虚拟内存
    pub fn unmap_memory(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
    ) -> MemoryResult<()>  {
        let target_range = VPNRange::new(start_va.floor(), end_va.ceil());
        if target_range.into_iter().any(|x|self.is_critical(x)) {
            return Err(AreaError::CriticalArea.into());
        }
        if self.is_unmapped(target_range) {
            return Err(AreaError::ContainUnmapped.into());
        }
        let areas = core::mem::take(&mut self.areas);
        for area in areas.into_iter() {
            let (l, _, rem) = area.get_vpn_range().exclude(&target_range);
            if rem.is_empty() {
                self.areas.push(area);
                continue;
            }
            let (larea, rarea) = area.split(l.get_end());
            let (mut marea, rarea) = rarea.split(rem.get_end());
            if !larea.get_vpn_range().is_empty() {
                self.areas.push(larea);
            }
            if !rarea.get_vpn_range().is_empty() {
                self.areas.push(rarea);
            }
            marea.unmap(&mut self.page_table)?;
            drop(marea);
        }
        Ok(())
    }
}

/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

/// remap test in kernel space
#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.exclusive_access();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert!(!kernel_space
        .page_table
        .translate(mid_text.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_rodata.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_data.floor())
        .unwrap()
        .executable(),);
    println!("remap_test passed!");
}