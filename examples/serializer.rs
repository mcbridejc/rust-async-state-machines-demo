/// Just an example of an object to be serialized. Each object has a type, and some arbitrary block
/// of bytes to describe it.
struct Object {
    pub object_type: u8,
    pub data: Vec<u8>,
}

mod async_serialize {
    use super::Object;
    use std::{cell::RefCell, pin::{pin, Pin}};
    use async_state_machine_example::poll_once;
    use futures::pending;


    struct AsyncSerializer<'a, 'b, F: Future<Output = ()>> {
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
    }

    impl<'a, 'b, F: Future<Output = ()>> PersistSerializer for AsyncSerializer<'a, 'b, F> {
        fn read(&mut self, buf: &mut [u8]) -> usize {
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

    /// Create a serializer for objects, and pass it to the provided callback
    ///
    /// This allows for pinning the required data on the stack for the duration of the serializer
    /// lifetime
    pub fn serialize_objects(objects: &[Object], mut cb: impl FnMut(&mut dyn PersistSerializer)) {
        let reg = RefCell::new(0u8);
        let fut = pin!(polling_serializer(objects, |b| *(&reg).borrow_mut() = b));
        let mut serializer = AsyncSerializer::new(fut, &reg);
        cb(&mut serializer);
    }

    pub trait PersistSerializer {
        fn read(&mut self, buf: &mut [u8]) -> usize;
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
    async fn polling_serializer(objects: &[Object], mut write_fn: impl FnMut(u8)) {
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
}

mod sync_serialize {
    use super::Object;

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
    pub struct SyncSerializer<'a> {
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
}

fn main() {
    // Some arbitrary sample data to serialize
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

    // A vec to store the fully written data
    let mut async_output = Vec::new();

    async_serialize::serialize_objects(&objects, |serializer| {
        // A temporary small buffer. Data will be serialized one 3-byte chunk at a time into this buffer.
        let mut buf = [0; 3];
        loop {
            let write_size = serializer.read(&mut buf);
            async_output.extend_from_slice(&buf[0..write_size]);
            if write_size < buf.len() {
                break;
            }
        }
    });

    // instantiate the sync state machine version
    let mut sync_serializer = sync_serialize::SyncSerializer::new(&objects);
    let mut sync_output = Vec::new();
    let mut buf = [0; 3];
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
