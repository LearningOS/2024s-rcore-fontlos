use core::fmt::Display;

use alloc::string::ToString;

/// Errors related to page permission
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

/// Errors related to page mapping
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

/// Errors related to area management
#[derive(Debug)]
pub enum AreaError {
    /// no requested area
    NoMatchingArea,
    /// requested area contains mapped portion,
    /// often returned from some mapping procsess.
    AreaHasMappedPortion,
    /// requested area contains unmapped portion,
    /// often returned from some unmapping process.
    AreaHasUnmappedPortion,
    /// when trying to unmap a critical area, e.g. `TRAMPOLINE`
    AreaCritical,
    /// when requested vpn is not inside the area
    AreaRangeNotInclude,
}

/// Errors related to memory management
#[derive(Debug)]
pub enum MMError {
    /// 内存不足
    MemoryNotEnough,
    /// 分页错误
    PageError(PageError),
    /// 内存区域错误
    AreaError(AreaError)
}

/// Wrapped `Result` for `MMError`
pub type MMResult<R> = core::result::Result<R, MMError>;

impl Display for MMError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MMError::MemoryNotEnough => f.write_str("NotEnoughMemory"),
            MMError::PageError(pe) => f.write_str(pe.to_string().as_str()),
            MMError::AreaError(ae) => f.write_str(ae.to_string().as_str()),
        }
    }
}

impl Display for PagePermissionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PagePermissionError::Unexecutable => f.write_str("NotExecutable"),
            PagePermissionError::Unreadable => f.write_str("NotReadable"),
            PagePermissionError::Unwritable => f.write_str("NotWritable"),
            PagePermissionError::Unaccessible => f.write_str("NotUserAccessible"),
        }
    }
}

impl Display for PageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PageError::DirPageInvalid => f.write_str("InvalidDirPage"),
            PageError::PageAlreadyAlloc => f.write_str("PageAlreadyValid"),
            PageError::PageInvalid => f.write_str("PageInvalid"),
            PageError::PermissionError(e) => f.write_str(e.to_string().as_str()),
        }
    }
}

impl Display for AreaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AreaError::NoMatchingArea => f.write_str("NoMatchingArea"),
            AreaError::AreaHasMappedPortion => f.write_str("AreaHasMappedPortion"),
            AreaError::AreaHasUnmappedPortion => f.write_str("AreaHasUnmappedPortion"),
            AreaError::AreaCritical => f.write_str("AreaCritical"),
            AreaError::AreaRangeNotInclude => f.write_str("AreaRangeNotInclude"),
        }
    }
}

impl From<PageError> for MMError {
    fn from(value: PageError) -> Self {
        Self::PageError(value)
    }
}
impl From<AreaError> for MMError {
    fn from(value: AreaError) -> Self {
        Self::AreaError(value)
    }
}
impl From<PagePermissionError> for MMError {
    fn from(value: PagePermissionError) -> Self {
        Self::PageError(PageError::PermissionError(value))
    }
}