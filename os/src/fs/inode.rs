//! `Arc<Inode>` -> `OSInodeInner`: In order to open files concurrently
//! we need to wrap `Inode` into `Arc`,but `Mutex` in `Inode` prevents
//! file systems from being accessed simultaneously
//!
//! `UPSafeCell<OSInodeInner>` -> `OSInode`: for static `ROOT_INODE`,we
//! need to wrap `OSInodeInner` into `UPSafeCell`
use alloc::collections::VecDeque;
use alloc::string::String;
use super::{File, Stat, StatMode};
use crate::drivers::BLOCK_DEVICE;
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::*;
use easy_fs::{EasyFileSystem, Inode};
use lazy_static::*;

/// inode in memory
/// A wrapper around a filesystem inode
/// to implement File trait atop
pub struct OSInode {
    readable: bool,
    writable: bool,
    stat: Stat,
    inner: UPSafeCell<OSInodeInner>,
}
/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

pub struct Link {
    target: String,
    link: String,
}

pub struct LinkManager{
    links: VecDeque<Arc<Link>>
}

impl OSInode {
    /// create a new inode in memory
    pub fn new(readable: bool, writable: bool, inode: Arc<Inode>, ino: u64, nlink: u32,stat_mode: StatMode) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
            stat: Stat::new(ino, nlink, stat_mode),
        }
    }
    /// read all data from the inode
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buffer = [0u8; 512];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

/// List all apps in the root directory
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    ///  The flags argument to the open() system call is constructed by ORing together zero or more of the following values:
    pub struct OpenFlags: u32 {
        /// readyonly
        const RDONLY = 0;
        /// writeonly
        const WRONLY = 1 << 0;
        /// read and write
        const RDWR = 1 << 1;
        /// create new file
        const CREATE = 1 << 9;
        /// truncate file size to 0
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

/// Open a file
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();

    let link_manager = LINK_MANAGER.exclusive_access();
    let (name, nlink, index)= link_manager.all(name);
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            // clear size
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode, index as u64, nlink as u32, StatMode::FILE)))
        } else {
            // create file
            ROOT_INODE
                .create(name)
                .map(|inode| Arc::new(OSInode::new(readable, writable, inode, index as u64, nlink as u32, StatMode::FILE)))
        }
    } else {
        ROOT_INODE.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode, index as u64, nlink as u32, StatMode::FILE))
        })
    }
}

impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
    fn stat(& self) -> Stat {
        self.stat.clone()
    }
}


impl LinkManager {
    pub fn new() -> Self {
        Self {
            links: VecDeque::new(),
        }
    }

    pub fn all<'a>(&'a self, name: &'a str) -> (&'a str, usize, usize) {
        let fetched_name = self.fetch(name);
        let nlink = self.find_num(&fetched_name);
        let index = self.find_index(&fetched_name);
        (fetched_name, nlink, index)
    }
    pub fn add(&mut self, target: &str, name: &str) -> isize {
        if target == name {
            return -1;
        }

        let link = Link {
            target: target.parse().unwrap(),
            link: name.parse().unwrap(),
        };
        self.links.push_back(Arc::from(link));
        0
    }

    pub fn remove(&mut self, name: &str) -> isize {
        let mut result: isize = -1;
        let mut indices_to_remove = Vec::new();

        for (index, link) in self.links.iter().enumerate() {
            let target = link.target.as_str();
            let link = link.link.as_str();
            if target == name || link == name {
                indices_to_remove.push(index);
                result = 0;
            }
        }
        for index in indices_to_remove.iter().rev() {
            self.links.remove(*index);
        }

        result
    }
    pub fn fetch<'a>(&'a self, name: &'a str) -> &'a str {
        if let Some(index) = self.links.iter().position(|link| {
            Arc::clone(link).target == name || Arc::clone(link).link == name
        }) {
            self.links[index].target.as_str()
        } else {
            println!("[Kernel][fs][inode]Not fetch the link in LINK_MANAGER");
            name
        }
    }

    pub fn find_num(&self, name: &str) -> usize {
        let count = self.links.iter().filter(|link| {
            Arc::clone(link).target == name
        }).count();

        if count == 0 {
            println!("[Kernel][fs][inode] Not fetch the link in LINK_MANAGER");
        }

        count + 1
    }

    pub fn find_index(&self, name: &str) -> usize {
        if let Some(index) = self.links.iter().position(|link_name| {
            Arc::clone(link_name).target == name
        }) {
            return index;
        } else {
            self.links.len()
        }
    }

}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref LINK_MANAGER: UPSafeCell<LinkManager> =
        unsafe { UPSafeCell::new(LinkManager::new()) };
}