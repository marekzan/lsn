//! user and group names for the metadata columns.
//! lookups go through libc and are cached, a tree shows the same few ids over
//! and over again.

use std::{cell::RefCell, collections::HashMap, ffi::CStr};

thread_local! {
    static USERS: RefCell<HashMap<u32, String>> = RefCell::new(HashMap::new());
    static GROUPS: RefCell<HashMap<u32, String>> = RefCell::new(HashMap::new());
}

pub fn user_name(uid: u32) -> String {
    USERS.with_borrow_mut(|cache| {
        cache
            .entry(uid)
            .or_insert_with(|| lookup_user(uid).unwrap_or_else(|| uid.to_string()))
            .clone()
    })
}

pub fn group_name(gid: u32) -> String {
    GROUPS.with_borrow_mut(|cache| {
        cache
            .entry(gid)
            .or_insert_with(|| lookup_group(gid).unwrap_or_else(|| gid.to_string()))
            .clone()
    })
}

fn lookup_user(uid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers point to memory we own and that lives until we
    // copied the name out of it
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(pwd.pw_name) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn lookup_group(gid: u32) -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut grp: libc::group = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::group = std::ptr::null_mut();
    // SAFETY: see lookup_user
    let rc = unsafe { libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(grp.gr_name) }
            .to_string_lossy()
            .into_owned(),
    )
}
