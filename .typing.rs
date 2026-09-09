    // What a hundred letters cost, which is what a person types in half a
    // minute — and what the undo history has to hold afterwards.
    let before = memory_in_use();
    let start = Instant::now();
    for _ in 0..100 {
        let at = wp_docx::TextPosition::new(middle, 0);
        document.set_caret(at);
        document.record_typing(at);
        document.insert_text(at, "x");
    }
    let hundred = start.elapsed();
    let after = memory_in_use();

