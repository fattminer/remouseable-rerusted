// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/remouseable.ico");
        resource
            .compile()
            .expect("failed to embed Windows application icon");
    }

    slint_build::compile("ui/remouseable.slint").expect("failed to compile Slint UI");
}
