use std::sync::Arc;

pub struct Ptr<T: Default> {
    instance: T,
}

impl<T: Default> Ptr<T> {
    pub fn new_arc() -> Arc<Ptr<T>> {
        Arc::new(Ptr {
            instance: T::default(),
        })
    }

    pub fn parse(instance: T) -> Arc<Ptr<T>> {
        Arc::new(Ptr { instance: instance })
    }

    pub fn get_mut(&self) -> &mut T {
        unsafe { &mut *(&self.instance as *const T as *mut T) }
    }
}

impl<T: Default> std::ops::Deref for Ptr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.instance
    }
}

#[cfg(test)]
pub mod test_us {}
