use bytes::{Buf, BufMut};

#[derive(Debug)]
pub struct BuffPool {
    data: Vec<u8>,
    offset: usize,
    max_buff_size: usize,
}

unsafe impl Send for BuffPool {}
unsafe impl Sync for BuffPool {}

impl BuffPool {
    pub fn new(capacity: usize) -> BuffPool {
        BuffPool {
            data: Vec::with_capacity(capacity),
            offset: 0,
            max_buff_size: capacity,
        }
    }
    /// 写入 需要写锁
    pub fn write(&mut self, data: &[u8]) {
        self.data.put_slice(data);
    }

    /// 读取 需要读锁
    pub fn read(&mut self) -> Result<Option<Vec<u8>>, &'static str> {
        let offset = self.offset;
        if offset + 4 > self.data.len() {
            return Ok(None);
        }
        let mut current = &self.data[offset..];
        let len = current.get_u32_le() as usize;
        if len > self.max_buff_size {
            return Err("buff len too long");
        }
        if len > current.len() {
            return Ok(None);
        }
        let data = current[..len].to_vec();
        self.offset = offset + len + 4;
        Ok(Some(data))
    }

    /// 挪数据 从屁股到头
    pub fn advance(&mut self) {
        if self.offset == 0 {
            return;
        }
        let len = self.data.len();
        unsafe {
            self.data.copy_within(self.offset.., 0);
            self.data.set_len(len - self.offset);
            self.offset = 0;
        }
    }

    pub fn reset(&mut self) {
        unsafe {
            self.data.set_len(0);
            self.offset = 0;
        }
    }
}
