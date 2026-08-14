use unicode_linebreak::{linebreaks, BreakOpportunity};
fn main() {
    for s in ["", "hello", "a\nb\nc\n", "word word word", "云龙风虎"] {
        let v: Vec<(usize, BreakOpportunity)> = linebreaks(s).collect();
        println!(
            "{:?} -> {:?}",
            s,
            v.iter()
                .map(|(i, k)| format!("{}:{:?}", i, k))
                .collect::<Vec<_>>()
        );
    }
}
