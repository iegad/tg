use std::{sync::Arc, fmt::Display};

use lockfree_object_pool::{LinearObjectPool, LinearReusable};


struct Person {
    name: String,
    age: u16,
}

impl Person {
    fn new() -> Self {
        Self {
            name: "".to_string(),
            age: 0,
        }
    }
}

impl Drop for Person {
    fn drop(&mut self) {
        println!("dropped...");
    }
}

impl Display for Person {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[name: {}, age: {}]\n", self.name, self.age)
    }
}

#[tokio::main]
async fn main () {
    let pool = Box::leak(Box::new(LinearObjectPool::new(Person::new, |v|{print!("BACKED: ----->>> {}", v)})));
    
    let (tx, mut wx) = tokio::sync::broadcast::channel::<Arc<LinearReusable<Person>>>(10);

    tokio::spawn(async move {
        for _ in 0..1000 {
            let p = Arc::new(pool.pull());
            if let Err(err) = tx.send(p) {
                println!("tx.send failed. {err}");
                break;
            }

            println!("...");
        }
    });

    while let Ok(p) = wx.recv().await {
        print!("{}", **p);
    }

    println!("done");
}