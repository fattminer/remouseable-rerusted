// This file is part of remouseable.
//
// remouseable is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 as published
// by the Free Software Foundation.

fn main() {
    slint_build::compile("ui/remouseable.slint").expect("failed to compile Slint UI");
}
