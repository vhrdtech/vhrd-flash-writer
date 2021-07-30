use core::ops::{Range, RangeInclusive};
use core::borrow::BorrowMut;
pub use stm32_device_signature;
use cfg_if::cfg_if;
use crate::mem_ext::MemExt;

/// First and Second keys witch must be written to unlock Flash
const KEY_1: u32 = 0x45670123;
const KEY_2: u32 = 0xCDEF89AB;

#[derive(Debug)]
pub enum FlashWriterError{
    InvalidRange,
    CannotGetFlashRegs,
    BsyTimeout,
    EraseFailed,
    FlashLocked,
    WriteFailed,
    WrongBankId,
    OutOfFlashWriterMemory,
    ProgErr,
    WrpErr,
    #[cfg(feature = "ext_errors")]
    SizeErr,
    #[cfg(feature = "ext_errors")]
    PgaErr,
    #[cfg(feature = "ext_errors")]
    PgsErr,
    #[cfg(feature = "ext_errors")]
    MissErr,
    #[cfg(feature = "ext_errors")]
    FastErr,
}


struct WriteBuff {
    data: [u8; PROGRAM_SIZE],
    len: usize
}

pub struct FlashWriter {
    #[cfg(target_os = "use_banks")]
    bank_change_on_page_num: u32,

    start_address: u32,
    end_address: u32,
    next_write_address: u32,
    image_len: usize,
    buffer: WriteBuff
}


fn check_range(range_cont: &mut RangeInclusive<u32>, range_check: &mut Range<u32>) -> bool {
    range_cont.contains(&range_check.start) && range_cont.contains(&range_check.end)
}

#[link_section = ".data"]
#[inline(never)]
fn check_errors_ram(regs: &mut FLASH) -> Result<(), FlashWriterError> {
    let sr = regs.sr.read();
    cfg_if! {
        if #[cfg(feature = "ext_errors")] {
            if sr.progerr().bit_is_set() { return Err(FlashWriterError::ProgErr); }
            if sr.sizerr().bit_is_set() { return Err(FlashWriterError::SizeErr); }
            if sr.pgaerr().bit_is_set() { return Err(FlashWriterError::PgaErr); }
            if sr.pgserr().bit_is_set() { return Err(FlashWriterError::PgsErr); }
            if sr.wrperr().bit_is_set() { return Err(FlashWriterError::WrpErr); }
            if sr.miserr().bit_is_set() { return Err(FlashWriterError::MissErr); }
            if sr.fasterr().bit_is_set() { return Err(FlashWriterError::FastErr); }
        }
        else{
            if sr.pgerr().bit_is_set() { return Err(FlashWriterError::ProgErr); }
            if sr.wrprt().bit_is_set() { return Err(FlashWriterError::WrpErr); }
        }
    }
    Ok(())
}

#[link_section = ".data"]
#[inline(never)]
fn check_bsy_sram(regs: &mut FLASH) -> Result<(), FlashWriterError> {
    let mut cnt: u16 = 0;
    while regs.sr.read().bsy().bit_is_set() || cnt < 220 { cnt += 1; }
    match regs.sr.read().bsy().bit_is_set() {
        true => { return Err(FlashWriterError::BsyTimeout); }
        false => {
            match check_errors_ram(regs){
                Ok(_) => { Ok(())}
                Err(e) => { Err(e)}
            }
        }
    }
}

#[link_section = ".data"]
#[inline(never)]
fn erase_sram(flash_writer: &mut FlashWriter, regs: &mut FLASH) -> Result<(), FlashWriterError> {
    for addr in (flash_writer.start_address..flash_writer.end_address).step_by(PAGE_SIZE) {
        regs.cr.modify(|_, w| w.per().set_bit());
        cfg_if! {
            if #[cfg(feature = "use_page_num")] {
                let mut page_number = ((addr - START_ADDR) / PAGE_SIZE as u32);
                if page_number > flash_writer.bank_change_on_page_num {
                    regs.cr.modify(|_,w|w.bker().set_bit());
                    page_number = (page_number - flash_writer.bank_change_on_page_num + 1u32) ;
                }
                else {
                    regs.cr.modify(|_,w|w.bker().clear_bit());
                }

                regs.cr.modify(|_, w| unsafe{ w.pnb().bits(page_number as u8) });
                regs.cr.modify(|_, w| w.start().set_bit());
            }
            else {
                 regs.ar.write(|w| unsafe { w.bits(addr) });
            }
        }
        cfg_if! {
            if #[cfg(feature = "start_bit")] {
                regs.cr.modify(|_, w| w.start().set_bit());
            }
            else {
                regs.cr.modify(|_, w| w.strt().set_bit());
            }
        }
        match check_bsy_sram(regs) {
            Err(e) => { return Err(e); }
            Ok(_) => {
                regs.cr.modify(|_, w| w.per().clear_bit());
                continue;
            }
        }
    }
    Ok(())
}
#[link_section = ".data"]
#[inline(never)]
fn write_sram(regs: &mut FLASH, address: u32, data: ProgramChunk) -> Result<(), FlashWriterError> {
    let w_a = address as *mut ProgramChunk;
    regs.cr.modify(|_, w| w.pg().set_bit());
    unsafe { core::ptr::write_volatile(w_a, data) };
    match check_bsy_sram(regs) {
        Err(e) => { return Err(e); }
        Ok(_) => { {
            regs.cr.modify(|_, w| w.pg().clear_bit());
            Ok(())
        }
        }
    }
}
///TODO move flash_regs to each function insted of owning it in struct
impl FlashWriter{
    pub fn new(mut range: Range<u32>) -> Result<self::FlashWriter, FlashWriterError> {
        let mut flash_range = START_ADDR..=START_ADDR + flash_size_bytes();
        match check_range(flash_range.borrow_mut(), range.borrow_mut()){
            true => {
                //regs.cr.modify(|_,w|w.eopie().set_bit());
                //unsafe{ regs.sr.modify(|_,w|w.bits(0x0000_0000)); };
                Ok(
                    FlashWriter{
                        #[cfg(target_os = "use_banks")]
                        bank_change_on_page_num: (stm32_device_signature::flash_size_kb() as u32 / (PAGE_SIZE * 2 / 1024 ) as u32) - 1u32,

                        start_address: range.start,
                        end_address: range.end,
                        next_write_address: range.start,
                        image_len: 0usize,
                        buffer: WriteBuff{
                            data: [0u8; PROGRAM_SIZE],
                            len: 0
                        }
                    })
            }
            false => { Err(FlashWriterError::InvalidRange)}
        }
    }
    pub fn erase(&mut self, regs: &mut FLASH) -> Result<(), FlashWriterError>{
        match self.unlock(regs){
            Err(e) => { return Err(e); }
            Ok(_) => {
                match erase_sram(self, regs){
                    Err(e) => { return Err(e); }
                    Ok(_) => {
                        self.lock(regs);
                        Ok(())
                    }
                }
            }
        }
    }

    pub fn get_start_address(&mut self) -> u32 {
        self.start_address
    }

    fn lock(&self, regs: &mut FLASH){
        regs.cr.modify(|_,w| w.lock().set_bit());
    }

    fn unlock(&self, regs: &mut FLASH) -> Result<(), FlashWriterError>{
        if regs.cr.read().lock().bit_is_clear(){
            return Ok(())
        }
        match check_bsy_sram(regs){
            Err(e) => { return Err(e); }
            Ok(_) => {
                if regs.cr.read().lock().bit_is_set() {
                    regs.keyr.write(|w|unsafe{w.bits(KEY_1)});
                    regs.keyr.write(|w|unsafe{w.bits(KEY_2)});
                }
                match regs.cr.read().lock().bit_is_clear(){
                    true => Ok(()),
                    false => Err(FlashWriterError::FlashLocked),
                }
            }
        }
    }
    pub fn write<T:Sized>(&mut self, regs: &mut FLASH, data_input: &[T]) -> Result<(), FlashWriterError> {
        match self.unlock(regs){
            Err(e) => { return Err(e); }
            Ok(_) => {
                let data = unsafe { core::slice::from_raw_parts(data_input.as_ptr() as *const u8, data_input.len() * core::mem::size_of::<T>()) };
                self.image_len += data.len();
                let mut len_to_take = 0usize;
                if self.buffer.len != 0 {
                    len_to_take = PROGRAM_SIZE - self.buffer.len;
                    let mut write_buf= [0xFF; PROGRAM_SIZE];
                    write_buf[0..self.buffer.len].copy_from_slice(&self.buffer.data[0..self.buffer.len]);
                    if data.len() >= len_to_take {
                        write_buf[self.buffer.len..self.buffer.len + len_to_take].copy_from_slice(&data[0..len_to_take]);
                    }
                    else {
                        write_buf[self.buffer.len..self.buffer.len + data.len()].copy_from_slice(&data[0..data.len()]);
                    }
                    self.buffer.len = 0;
                    let mut dat = 0 as ProgramChunk;
                    unsafe {
                        core::ptr::copy_nonoverlapping(write_buf.as_ptr(),
                                                       &mut dat as *mut _ as *mut u8,
                                                       PROGRAM_SIZE)
                    };
                    match write_sram(regs, self.next_write_address, dat) {
                        Ok(_) => {
                            if self.next_write_address <= (self.end_address - PROGRAM_SIZE as u32) {
                                self.next_write_address += PROGRAM_SIZE as u32;
                            }
                            else{
                                return Err(FlashWriterError::OutOfFlashWriterMemory);
                            }
                        }
                        Err(e) => { return Err(e); }
                    }

                }

                if data.len() > len_to_take {
                    let chunks = data[len_to_take..data.len()].chunks_exact(PROGRAM_SIZE);
                    let remainder = chunks.remainder();

                    for bytes in chunks.into_iter() {
                        let mut dat = 0 as ProgramChunk;
                        unsafe {
                            core::ptr::copy_nonoverlapping(bytes.as_ptr(),
                                                           &mut dat as *mut _ as *mut u8,
                                                           PROGRAM_SIZE)
                        };
                        match write_sram(regs, self.next_write_address, dat) {
                            Ok(_) => {
                                if self.next_write_address <= (self.end_address - PROGRAM_SIZE as u32) {
                                    self.next_write_address += PROGRAM_SIZE as u32;
                                }
                                else{
                                    return Err(FlashWriterError::OutOfFlashWriterMemory);
                                }
                            }
                            Err(e) => { return Err(e); }
                        }
                    }
                    self.buffer.data[0..remainder.len()].copy_from_slice(remainder);
                    self.buffer.len = remainder.len();
                }
                self.lock(regs);
                Ok(())
            }
        }
    }

    pub fn flush(&mut self, regs: &mut FLASH) -> Result<(), FlashWriterError> {
        if self.buffer.len != 0 {
            let mut dat = ProgramChunk::max_value();
            for i in 0..self.buffer.len{
                dat = dat << 8 | self.buffer.data[self.buffer.len - 1 - i] as ProgramChunk;
            }
            if self.next_write_address < (self.end_address - PROGRAM_SIZE as u32) {
                match write_sram(regs, self.next_write_address, dat) {
                    Ok(_) => {
                        self.buffer.len = 0;
                        self.lock(regs);
                        Ok(())
                    }
                    Err(e) => {
                        self.lock(regs);
                        return Err(e);
                    }
                }
            }
            else {
                return Err(FlashWriterError::OutOfFlashWriterMemory);
            }
        }
        else {
            self.lock(regs);
            Ok(())
        }
    }
}

pub fn flash_read_slice<T:Sized>(addr: u32, len_to_read: usize) -> &'static [T] {
    unsafe { core::slice::from_raw_parts(addr as *const T, len_to_read) }
}

pub fn flash_read<T:Sized>(addr: u32) -> T {
    unsafe { core::ptr::read_volatile(addr as * const T) }
}

pub fn flash_size_bytes() -> u32{
    (stm32_device_signature::flash_size_kb() as u32).kb()
}

cfg_if!{
 if #[cfg(feature = "stm32f0xx")]{
        type ProgramChunk = u16;
        const START_ADDR: u32 = 0x0800_0000;
        const PAGE_SIZE: usize = 1024;
        const PROGRAM_SIZE: usize = core::mem::size_of::<ProgramChunk>();
        use stm32f0xx_hal::stm32::FLASH;
    }
}
