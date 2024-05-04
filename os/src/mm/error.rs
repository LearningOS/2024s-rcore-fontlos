use core::fmt::Display;

use alloc::string::ToString;

/// 内存分页权限错误
#[derive(Debug)]
pub enum PagePermissionError {
    /// 不可执行
    Unexecutable,
    /// 不可读
    Unreadable,
    /// 不可写
    Unwritable,
    /// 用户无权访问
    Unaccessible,
}

/// 内存页映射错误
#[derive(Debug)]
pub enum PageError {
    /// 无效的目录页
    DirPageInvalid,
    /// 页已分配
    PageAlreadyAlloc,
    /// 无效页
    PageInvalid,
    /// 权限错误
    PermissionError(PagePermissionError)
}

/// 内存区域分配错误
#[derive(Debug)]
pub enum AreaError {
    /// 内存区域不匹配
    NotMatch,
    /// 内存区域超出范围
    NotInclude,
    /// 包含以映射区域
    ContainMapped,
    /// 包含未映射部分
    ContainUnmapped,
    /// 无法回收关键映射区域
    CriticalArea,
}

/// 内存错误
#[derive(Debug)]
pub enum MemoryError {
    /// 内存不足
    MemoryNotEnough,
    /// 分页错误
    PageError(PageError),
    /// 内存区域错误
    AreaError(AreaError)
}

/// 对内存错误的包装
pub type MemoryResult<R> = core::result::Result<R, MemoryError>;

impl Display for MemoryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MemoryError::MemoryNotEnough => f.write_str("MemoryNotEnough"),
            MemoryError::PageError(pe) => f.write_str(pe.to_string().as_str()),
            MemoryError::AreaError(ae) => f.write_str(ae.to_string().as_str()),
        }
    }
}

impl Display for PagePermissionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PagePermissionError::Unexecutable => f.write_str("Unexecutable"),
            PagePermissionError::Unreadable => f.write_str("Unreadable"),
            PagePermissionError::Unwritable => f.write_str("Unwritable"),
            PagePermissionError::Unaccessible => f.write_str("Unaccessible"),
        }
    }
}

impl Display for PageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PageError::DirPageInvalid => f.write_str("DirPageInvalid"),
            PageError::PageAlreadyAlloc => f.write_str("PageAlreadyAlloc"),
            PageError::PageInvalid => f.write_str("PageInvalid"),
            PageError::PermissionError(e) => f.write_str(e.to_string().as_str()),
        }
    }
}

impl Display for AreaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AreaError::NotMatch => f.write_str("NotMatch"),
            AreaError::NotInclude => f.write_str("NotInclude"),
            AreaError::ContainMapped => f.write_str("ContainMapped"),
            AreaError::ContainUnmapped => f.write_str("ContainUnmapped"),
            AreaError::CriticalArea => f.write_str("CriticalArea"),
        }
    }
}

impl From<PageError> for MemoryError {
    fn from(value: PageError) -> Self {
        Self::PageError(value)
    }
}
impl From<AreaError> for MemoryError {
    fn from(value: AreaError) -> Self {
        Self::AreaError(value)
    }
}
impl From<PagePermissionError> for MemoryError {
    fn from(value: PagePermissionError) -> Self {
        Self::PageError(PageError::PermissionError(value))
    }
}