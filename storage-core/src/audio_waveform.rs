pub fn extract_peaks(samples: &[f32], target_bins: usize) -> Vec<f32> {
    if samples.is_empty() || target_bins == 0 {
        return vec![];
    }
    let total = samples.len();
    let mut out = Vec::with_capacity(target_bins);
    for i in 0..target_bins {
        let start = (i * total) / target_bins;
        let end = (((i + 1) * total) / target_bins).min(total).max(start + 1);
        let max_val = samples[start..end]
            .iter()
            .fold(0.0f32, |a, &b| a.max(b.abs()));
        out.push(max_val);
    }
    out
}

pub fn extract_peaks_u8(samples: &[f32], target_bins: usize) -> Vec<u8> {
    extract_peaks(samples, target_bins)
        .into_iter()
        .map(|p| (p * 255.0) as u8)
        .collect()
}
