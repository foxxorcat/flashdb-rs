use crate::{
  fdb_kvdb_control, fdb_kvdb_t, fdb_tsdb_control,
    fdb_tsdb_t, 
};


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
