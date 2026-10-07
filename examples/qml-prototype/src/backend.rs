use cxx_qt_lib::QString;

#[cxx_qt::bridge]
mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, greeting)]
        type Backend = super::BackendRust;

        #[qinvokable]
        #[cxx_name = "say_hello"]
        fn say_hello(&self);
    }
}

pub struct BackendRust {
    greeting: QString,
}

impl Default for BackendRust {
    fn default() -> Self {
        Self {
            greeting: QString::from("Hello from Rust via cxx-qt!"),
        }
    }
}

impl qobject::Backend {
    pub fn say_hello(&self) {
        println!("Hello World from the QML bridge!");
    }
}
