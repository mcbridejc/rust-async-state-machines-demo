use futures::pending;
use core::future::Future;
use std::{cell::RefCell, pin::{pin, Pin}};


use async_state_machine_example::poll_once;

/// Just an example of an object to be serialized. Each object has a type, and some arbitrary block
/// of bytes to describe it.
struct Object {
    pub object_type: u8,
    pub data: Vec<u8>,
}

pub struct AsyncSerializer<'a, 'b, F: Future<Output = ()>> {
    fut: Pin<&'a mut F>,
    reg: &'b RefCell<u8>,
}

impl<'a, 'b, F> AsyncSerializer<'a, 'b, F>
where
    F: Future<Output = ()>
{
    /// Create a serializer using a future and a shared data buffer
    ///
    /// The future should write to reg, and it *must yield after writing each byte*.
    pub fn new(fut: Pin<&'a mut F>, reg: &'b RefCell<u8>) -> Self {
        Self { fut, reg }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut pos = 0;
        loop {
            if pos >= buf.len() {
                return pos;
            }

            if let Some(_) = poll_once(self.fut.as_mut()) {
                // Serialization is complete
                return pos;
            } else {
                buf[pos] = *self.reg.borrow();
                pos += 1;
            }
        }
    }
}


/// Write bytes to a provided function, returning a Poll::Pending between each byte
async fn write_bytes(src: &[u8], mut write_fn: impl FnMut(u8)) {
    for i in 0..src.len() {
        write_fn(src[i]);
        pending!()
    }
}

/// Implements a serializer for a list of objects.
///
/// Implementing this as an async function allows the sequence of writes to be written linearly and
/// simply, allowing rustc to compile it into a state machine so that the serialization can be
/// broken up into arbitrary chunks
async fn polling_write(objects: &[Object], mut write_fn: impl FnMut(u8)) {
    for obj in objects {
        // First serialize the size of the object, which is the length of the data + 1 byte for the
        // object type
        let len = (obj.data.len() + 1) as u16;
        write_bytes(&len.to_le_bytes(), &mut write_fn).await;
        // Serialize the object type
        write_bytes(&[obj.object_type], &mut write_fn).await;
        // Serialize the object data
        write_bytes(&obj.data, &mut write_fn).await;
    }
}

pub enum State {
    Len1,
    Len2,
    Type,
    Data(usize),
}

impl State {
    pub fn new() -> Self {
        State::Len1
    }
}

/// A synchronous state machine implementation of the serialization
///
/// It has to explicitly keep track of its position in the serialization so that it can stop and
/// resume at any byte in the output
struct SyncSerializer<'a> {
    objects: &'a [Object],
    state: State,
    idx: usize,
}

impl<'a> SyncSerializer<'a> {
    pub fn new(objects: &'a [Object]) -> Self {
        Self { objects, state: State::new(), idx: 0 }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut pos = 0;
        loop {
            if pos >= buf.len() {
                return pos;
            }
            if self.idx >= self.objects.len() {
                return pos;
            }
            let obj = &self.objects[self.idx];
            self.state = match self.state {
                State::Len1 => {
                    let len = ((obj.data.len() + 1) as u16).to_le_bytes();
                    buf[pos] = len[0];
                    State::Len2
                },
                State::Len2 => {
                    let len = ((obj.data.len() + 1) as u16).to_le_bytes();
                    buf[pos] = len[1];
                    State::Type
                },
                State::Type => {
                    buf[pos] = obj.object_type;
                    if obj.data.len() > 0 {
                        State::Data(0)
                    } else {
                        self.idx += 1;
                        State::Len1
                    }
                },
                State::Data(offset) => {
                    buf[pos] = obj.data[offset];
                    if offset >= obj.data.len() - 1 {
                        self.idx += 1;
                        State::Len1
                    } else {
                        State::Data(offset + 1)
                    }
                },
            };
            pos += 1;
        }
    }
}

fn main() {
    let objects = vec![
        Object {
            object_type: 0,
            data: vec![1,2,3,4]
        },
        Object {
            object_type: 1,
            data: vec![1,2,3,4,5,6,7,8],
        }
    ];

    // `byte_buf` serves as a temporary register for communication between the future which implements
    // serialization and the Serializer struct
    let byte_buf = RefCell::new(0u8);
    // Create the future from an async function, each time it is polled, it will write one byte to
    // `byte_buf`
    let future = polling_write(&objects, |b| *byte_buf.borrow_mut() = b);
    let future = pin!(future);
    // instantiate the wrapper which will drive the future
    let mut async_serializer = AsyncSerializer::new(future, &byte_buf);
    // instantiate the sync state machine version
    let mut sync_serializer = SyncSerializer::new(&objects);

    // A vec to store the fully written data
    let mut async_output = Vec::new();
    let mut sync_output = Vec::new();
    // A temporary small buffer. Data will be serialized one 3-byte chunk at a time into this buffer.
    let mut buf = [0; 3];
    loop {
        let write_size = async_serializer.read(&mut buf);
        async_output.extend_from_slice(&buf[0..write_size]);
        if write_size < buf.len() {
            break;
        }
    }
    loop {
        let write_size = sync_serializer.read(&mut buf);
        sync_output.extend_from_slice(&buf[0..write_size]);
        if write_size < buf.len() {
            break;
        }
    }

    assert_eq!(async_output, [5,0,0,1,2,3,4, 9,0,1,1,2,3,4,5,6,7,8]);
    assert_eq!(sync_output, async_output);
}
