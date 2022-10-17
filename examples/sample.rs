use std::ptr;

struct Person {
    _name: String,
    _age: u16,
}

impl Drop for Person {
    fn drop(&mut self) {
        println!("dropped...");
    }
}

fn main () {
    let p = Box::new(Person{_name: "".to_string(), _age: 10});
    
    {
        let p = Box::leak(p);
        unsafe { ptr::drop_in_place(p as *mut Person); }
    }
    
    println!("-------------------->>>");
}