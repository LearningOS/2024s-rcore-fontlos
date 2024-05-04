use alloc::collections::BTreeMap;

use crate::{config::PAGE_SIZE, mm::address::StepByOne};
use super::{frame_alloc, FrameTracker, MemoryError, MemoryResult, PageError, PhysPageNum, VirtAddr, VirtPageNum};
use super::address::VPNRange;
use super::page_table::{PTEFlags, PageTable};

/// 连续虚拟内存映射
pub struct MapArea {
    vpn_range: VPNRange,
    data_frame: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_permission: MapPermission,
}

impl MapArea {
    /// 或许虚拟页码范围
    pub fn get_vpn_range(&self) -> VPNRange {
        self.vpn_range
    }

    /// 分割内存映射
    pub fn split(self, vpn: VirtPageNum) -> (Self, Self) {
        let mut other = Self {vpn_range: VPNRange::new(vpn, vpn), data_frame: BTreeMap::new(), map_type: self.map_type, map_permission: self.map_permission};
        if vpn <= self.vpn_range.get_start() {
            return (other, self);
        } else if vpn >= self.vpn_range.get_end() {
            return (self, other);
        } else {
            let mut left = BTreeMap::new();
            let mut right = BTreeMap::new();
            for (i, frame) in self.data_frame.into_iter() {
                if i < vpn {
                    left.insert(i, frame);
                } else {
                    right.insert(i, frame);
                }
            }
            let left = Self {
                vpn_range: VPNRange::new(self.vpn_range.get_start(), vpn),
                data_frame: left,
                map_type: self.map_type,
                map_permission: self.map_permission
            };
            other = Self {
                vpn_range: VPNRange::new(vpn, self.vpn_range.get_end()),
                data_frame: right,
                map_type: self.map_type,
                map_permission: self.map_permission
            };
            return (left, other);
        }
    }
    /// 新建映射区域
    pub fn new(
        start_virt_addr: VirtAddr,
        end_virt_addr: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_virt_addr.floor();
        let end_vpn: VirtPageNum = end_virt_addr.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frame: BTreeMap::new(),
            map_type,
            map_permission: map_perm,
        }
    }

    /// 检查页的原始函数
    fn check_page_raw(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) -> MemoryResult<()> {
        if !self.data_frame.contains_key(&vpn) {
            let frame = frame_alloc().ok_or(MemoryError::MemoryNotEnough)?;
            let ppn = frame.ppn;
            self.data_frame.insert(vpn, frame);
            let pte_flags = PTEFlags::from_bits(self.map_permission.bits).unwrap();
            match page_table.map(vpn, ppn, pte_flags) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    self.data_frame.remove(&vpn);
                    return Err(e);
                },
            }
        }
        Ok(())
    }
    /// 检查范围
    pub fn check_range(&mut self, page_table: &mut PageTable, vpn_range: VPNRange) -> MemoryResult<()> {
        match self.map_type {
            MapType::Identical => Ok(()),
            MapType::Framed => {
                self.vpn_range.intersection(&vpn_range);
                for vpn in self.vpn_range.intersection(&vpn_range) {
                    self.check_page_raw(page_table, vpn)?;
                }
                Ok(())
            },
        }
    }
    /// 检查所有要映射的页
    pub fn check_all_page(&mut self, page_table: &mut PageTable) -> MemoryResult<()> {
        self.check_range(page_table, self.vpn_range)
    }

    fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) -> MemoryResult<()> {
        match self.map_type {
            MapType::Identical => {
                let ppn = PhysPageNum(vpn.0);
                let pte_flags = PTEFlags::from_bits(self.map_permission.bits).unwrap();
                page_table.map(vpn, ppn, pte_flags)
            }
            MapType::Framed => {
                Ok(())
            }
        }
    }

    fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) -> MemoryResult<()> {
        if self.map_type == MapType::Framed {
            self.data_frame.remove(&vpn); // 释放映射的帧
        }
        match page_table.unmap(vpn) {
            Ok(_) => Ok(()),
            Err(MemoryError::PageError(PageError::DirPageInvalid)) => Ok(()),
            Err(MemoryError::PageError(PageError::PageInvalid)) => Ok(()),
            Err(e) => Err(e)
        }
    }
    /// 非严格的完全映射
    pub fn map(&mut self, page_table: &mut PageTable) -> MemoryResult<()> {
        for vpn in self.vpn_range {
            match self.map_one(page_table, vpn) {
                Ok(_) => {},
                Err(e) => return Err(e)
            }
        }
        Ok(())
    }

    /// 取消所有映射
    pub fn unmap(&mut self, page_table: &mut PageTable) -> MemoryResult<()> {
        for vpn in self.vpn_range {
            match self.unmap_one(page_table, vpn) {
                Ok(_) => {},
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// 收缩内存区域
    #[allow(unused)]
    pub fn narrow(&mut self, page_table: &mut PageTable, to: VirtPageNum) -> MemoryResult<()> {
        for vpn in VPNRange::new(to, self.vpn_range.get_end()) {
            match self.unmap_one(page_table, vpn) {
                Ok(_) => {},
                Err(e) => return Err(e)
            }
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), to);
        Ok(())
    }

    /// 扩张内存区域
    #[allow(unused)]
    pub fn expand(&mut self, page_table: &mut PageTable, to: VirtPageNum) -> MemoryResult<()> {
        for vpn in VPNRange::new(self.vpn_range.get_end(), to) {
            match self.map_one(page_table, vpn) {
                Ok(_) => {},
                Err(e) => return Err(e)
            }
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), to);
        Ok(())
    }
    /// 复制数据并确保所需要的帧
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) -> MemoryResult<()> {
        assert_eq!(self.map_type, MapType::Framed);
        let pages = (data.len() - 1 + PAGE_SIZE) / PAGE_SIZE;
        assert!(pages <= self.vpn_range.into_iter().count());
        self.check_range(page_table, VPNRange::new_by_len(self.vpn_range.get_start(), pages))?;
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)?
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
        Ok(())
    }
}

/// 内存映射类型
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MapType {
    /// 特定映射
    Identical,
    /// 帧映射
    Framed,
}

bitflags! {
    /// 将权限映射到 R W X U
    pub struct MapPermission: u8 {
        /// 可读
        const R = 1 << 1;
        /// 可写
        const W = 1 << 2;
        /// 可执行
        const X = 1 << 3;
        /// 用户可操作
        const U = 1 << 4;
    }
}
