use crate::{
    fdb_blob, fdb_blob__bindgen_ty_1, fdb_kv, fdb_kvdb_control, fdb_kvdb_t, fdb_tsdb_control,
    fdb_tsdb_t, fdb_tsl,
};

pub fn fdb_blob_make_by(v: &mut [u8], kv: &fdb_kv, offset: usize) -> fdb_blob {
    fdb_blob {
        buf: v.as_mut_ptr() as *mut _,
        size: v.len(),
        saved: fdb_blob__bindgen_ty_1 {
            meta_addr: kv.addr.start,
            addr: kv.addr.value + offset as u32,
            len: kv.value_len as usize - offset,
        },
    }
}

pub fn fdb_blob_make_read(v: &mut [u8]) -> fdb_blob {
    fdb_blob {
        buf: v.as_mut_ptr() as *mut _,
        size: v.len(),
        saved: unsafe { core::mem::zeroed() },
    }
}
pub fn fdb_blob_make_write(v: &[u8]) -> fdb_blob {
    fdb_blob {
        buf: v.as_ptr() as *const _ as *mut _,
        size: v.len(),
        saved: unsafe { core::mem::zeroed() },
    }
}

pub fn fdb_blob_make_by_tsl(v: &mut [u8], tsl: &fdb_tsl, offset: usize) -> fdb_blob {
    fdb_blob {
        buf: v.as_mut_ptr() as *mut _,
        size: v.len(),
        saved: fdb_blob__bindgen_ty_1 {
            meta_addr: tsl.addr.index,
            addr: tsl.addr.log + offset as u32,
            len: tsl.log_len as usize - offset,
        },
    }
}

pub fn fdb_kvdb_control_write<T>(db: fdb_kvdb_t, cmd: u32, arg: T) {
    unsafe { fdb_kvdb_control(db, cmd as i32, &arg as *const _ as *mut _) }
}

pub fn fdb_kvdb_control_read<T>(db: fdb_kvdb_t, cmd: u32, arg: &mut T) {
    unsafe { fdb_kvdb_control(db, cmd as i32, arg as *mut _ as *mut _) }
}

pub fn fdb_tsdb_control_write<T>(db: fdb_tsdb_t, cmd: u32, arg: T) {
    unsafe { fdb_tsdb_control(db, cmd as i32, &arg as *const _ as *mut _) }
}

pub fn fdb_tsdb_control_read<T>(db: fdb_tsdb_t, cmd: u32, arg: &mut T) {
    unsafe { fdb_tsdb_control(db, cmd as i32, arg as *mut _ as *mut _) }
}
