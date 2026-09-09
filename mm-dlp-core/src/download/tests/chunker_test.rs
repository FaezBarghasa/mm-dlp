use crate::download::chunker::{calculate_chunks, assemble_chunks, CHUNK_SIZE};
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use std::path::Path;

#[test]
fn test_calculate_chunks() {
    let total_size = CHUNK_SIZE * 3 + 100;
    let chunks = calculate_chunks(total_size);
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0], (0, CHUNK_SIZE - 1));
    assert_eq!(chunks[1], (CHUNK_SIZE, CHUNK_SIZE * 2 - 1));
    assert_eq!(chunks[2], (CHUNK_SIZE * 2, CHUNK_SIZE * 3 - 1));
    assert_eq!(chunks[3], (CHUNK_SIZE * 3, total_size - 1));
}

#[tokio::test]
async fn test_assemble_chunks() {
    let dest_path = Path::new("/tmp/test_assemble_chunks");
    let part_dir = dest_path.with_extension("part");
    fs::create_dir_all(&part_dir).await.unwrap();

    let mut total_size = 0;
    for i in 0..3 {
        let chunk_path = part_dir.join(format!("chunk_{}", i));
        let mut chunk_file = File::create(&chunk_path).await.unwrap();
        let content = format!("chunk {}", i);
        chunk_file.write_all(content.as_bytes()).await.unwrap();
        total_size += content.len() as u64;
    }

    assemble_chunks(dest_path, total_size).await.unwrap();

    let final_content = fs::read_to_string(dest_path).await.unwrap();
    assert_eq!(final_content, "chunk 0chunk 1chunk 2");

    fs::remove_file(dest_path).await.unwrap();
}
