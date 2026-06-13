# mm-dlp Developer Guide

Welcome to the developer documentation for **mm-dlp**. This guide is intended for contributors who want to understand the internal architecture, add new platform extractors, or improve the core engine.

## Architecture Overview

At its heart, `mm-dlp-core` is a routing and parsing engine. The core architectural philosophy is **composition and trait-based dispatch**.

Instead of writing a massive, monolithic function with endless `if/else` statements for every supported domain, the project is split into discrete **Extractors**. Each platform gets its own extractor that knows exactly how to parse URLs for that specific service.

### Key Components

1. **`PlatformRegistry`**: The central router. It holds a collection of boxed extractors (`Vec<Box<dyn PlatformExtractor>>`). When `registry.extract(url)` is called, it iterates through its registered extractors until one successfully returns `MediaMetadata`.
2. **`PlatformExtractor`**: A thread-safe trait (`Send + Sync`) that requires a single method: `extract(&self, url: &str) -> Option<MediaMetadata>`.
3. **`MediaMetadata`**: The standardized data structure returned upon successful extraction.
4. **`clean_id` Helper**: A utility function provided by the core library to strip tracking tags (e.g., `?si=...`), query parameters, and trailing slashes from raw ID strings.

---

## Concurrency and Thread Safety

Because `mm-dlp` is built to be a high-performance downloader, the `PlatformExtractor` trait enforces `Send + Sync`. 

When implementing new features or extractors:
- **Avoid interior mutability** (like `RefCell` or `Mutex`) within extractors unless absolutely necessary. Extractors should ideally be stateless URL parsers.
- **Do not use thread-local state**, as the registry might be shared across a thread pool (e.g., using `rayon` or `tokio`) to process thousands of URLs concurrently.

---

## Contributor Rules & Code Standards

To maintain a production-ready and highly reliable codebase, contributors must strictly adhere to the following rules:
- **No Function Stubs**: Every function must be completely implemented and fully functional. Do not make function stubs or use empty placeholders.
- **No `todo!()` or `FIXME`**: Code must be finalized before submitting. Do not leave `TODO`, `FIXME`, or change functionality to `todo!()` anywhere in the repository.
- **Robust Error Handling**: Properly process all `Result` and `Option` types. Avoid using `.unwrap()` or `.expect()` in library implementation code outside of testing modules.

---

## Adding a New Platform Extractor

Adding a new platform is straightforward. Let's walk through adding a fully functional extractor for a hypothetical platform called **EchoStream**.

### 1. Create the Extractor File
Create a new file in the `platforms` directory, for example: `src/platforms/echostream.rs`.

### 2. Implement the Trait
Write the extractor logic. Make sure to handle URL edge cases and utilize `clean_id`.

```rust
use crate::core::{MediaMetadata, PlatformExtractor, clean_id};

pub struct EchoStreamExtractor;

impl PlatformExtractor for EchoStreamExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        // Ensure the URL belongs to EchoStream
        if !url.contains("echostream.tv/") {
            return None;
        }

        // Parse Video URLs: echostream.tv/watch/v/12345abcd
        if let Some(watch_idx) = url.find("/watch/v/") {
            let raw_id = &url[watch_idx + 9..];
            let id = clean_id(raw_id);
            
            if !id.is_empty() {
                return Some(MediaMetadata {
                    platform: "EchoStream".to_string(),
                    media_type: "Video".to_string(),
                    media_id: id,
                });
            }
        }

        // Parse Live Stream URLs: echostream.tv/live/channelname
        if let Some(live_idx) = url.find("/live/") {
            let raw_id = &url[live_idx + 6..];
            let id = clean_id(raw_id);
            
            if !id.is_empty() {
                return Some(MediaMetadata {
                    platform: "EchoStream".to_string(),
                    media_type: "Live Stream".to_string(),
                    media_id: id,
                });
            }
        }

        None
    }
}
```

### 3. Register the Extractor
Open the file where `PlatformRegistry` is defined (likely `src/platforms/mod.rs` or `registry.rs`). 
Add your new extractor to the initialization vector.

```rust
// Inside src/platforms/registry.rs (or similar)

use crate::platforms::echostream::EchoStreamExtractor;
// ... other imports ...

impl PlatformRegistry {
    pub fn new() -> Self {
        let mut extractors: Vec<Box<dyn PlatformExtractor>> = Vec::new();
        
        // Add existing extractors
        extractors.push(Box::new(YouTubeExtractor));
        extractors.push(Box::new(SpotifyExtractor));
        
        // Register your new extractor here
        extractors.push(Box::new(EchoStreamExtractor));
        
        PlatformRegistry { extractors }
    }
}
```

---

## Testing Your Extractor

We strictly enforce unit testing for every platform. When adding an extractor, you must provide tests that cover standard URLs, URLs with query parameters, and invalid URLs.

Append this standard test module to the bottom of your `echostream.rs` file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echostream_video_extraction() {
        let extractor = EchoStreamExtractor;
        
        // Standard URL
        let meta = extractor.extract("https://echostream.tv/watch/v/98765xyz").unwrap();
        assert_eq!(meta.platform, "EchoStream");
        assert_eq!(meta.media_type, "Video");
        assert_eq!(meta.media_id, "98765xyz");

        // URL with trailing query parameters
        let meta_dirty = extractor.extract("https://echostream.tv/watch/v/98765xyz?autoplay=1&ref=twitter").unwrap();
        assert_eq!(meta_dirty.media_id, "98765xyz"); // clean_id should have stripped the query
    }

    #[test]
    fn test_echostream_live_extraction() {
        let extractor = EchoStreamExtractor;
        let meta = extractor.extract("https://echostream.tv/live/gaming_channel/").unwrap();
        assert_eq!(meta.media_type, "Live Stream");
        assert_eq!(meta.media_id, "gaming_channel"); // clean_id should have stripped the trailing slash
    }

    #[test]
    fn test_echostream_invalid_url() {
        let extractor = EchoStreamExtractor;
        assert!(extractor.extract("https://echostream.tv/about-us").is_none());
    }
}
```

---

## Language Bindings & Usage Examples

`mm-dlp` can be used from various programming languages through its FFI bindings. The core function exposed is `extract_metadata`, which takes a URL and returns a JSON string containing the extracted `MediaMetadata`.

Below are examples for each supported language.

### Python

```python
import ctypes
import json

# Load the shared library
lib = ctypes.CDLL("./libmm_dlp.so")

# Define the function signature
lib.extract_metadata.argtypes = [ctypes.c_char_p]
lib.extract_metadata.restype = ctypes.c_char_p

def extract(url):
    result_ptr = lib.extract_metadata(url.encode('utf-8'))
    if result_ptr:
        result_json = ctypes.string_at(result_ptr).decode('utf-8')
        # Remember to free the string memory from Rust
        lib.free_string(result_ptr)
        return json.loads(result_json)
    return None

# Example usage
url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
metadata = extract(url)
if metadata:
    print(f"Platform: {metadata['platform']}, ID: {metadata['media_id']}")
```

### JavaScript (Node.js)

```javascript
const ffi = require('ffi-napi');
const ref = require('ref-napi');

// Define the C string type
const charPtr = ref.refType('char');

const lib = ffi.Library('libmm_dlp', {
  'extract_metadata': [charPtr, ['string']],
  'free_string': ['void', [charPtr]]
});

function extract(url) {
  const resultPtr = lib.extract_metadata(url);
  if (!resultPtr.isNull()) {
    const resultJson = ref.readCString(resultPtr, 0);
    lib.free_string(resultPtr);
    return JSON.parse(resultJson);
  }
  return null;
}

// Example usage
const url = "https://twitter.com/jack/status/20";
const metadata = extract(url);
if (metadata) {
  console.log(`Platform: ${metadata.platform}, ID: ${metadata.media_id}`);
}
```

### Java

```java
import com.sun.jna.Library;
import com.sun.jna.Native;
import com.sun.jna.Pointer;

public class MmDlp {

    public interface MmDlpLib extends Library {
        MmDlpLib INSTANCE = Native.load("mm_dlp", MmDlpLib.class);

        Pointer extract_metadata(String url);
        void free_string(Pointer ptr);
    }

    public static String extract(String url) {
        Pointer resultPtr = MmDlpLib.INSTANCE.extract_metadata(url);
        if (resultPtr != null) {
            String resultJson = resultPtr.getString(0);
            MmDlpLib.INSTANCE.free_string(resultPtr);
            return resultJson;
        }
        return null;
    }

    public static void main(String[] args) {
        String url = "https://www.instagram.com/p/CXYZ123abc/";
        String metadataJson = extract(url);
        if (metadataJson != null) {
            // Using a JSON library like Gson or Jackson is recommended
            System.out.println(metadataJson);
        }
    }
}
```

### Kotlin

```kotlin
import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer

interface MmDlpLib : Library {
    fun extract_metadata(url: String): Pointer?
    fun free_string(ptr: Pointer)
}

object MmDlp {
    private val lib = Native.load("mm_dlp", MmDlpLib::class.java)

    fun extract(url: String): String? {
        val resultPtr = lib.extract_metadata(url)
        return resultPtr?.let {
            val json = it.getString(0)
            lib.free_string(it)
            json
        }
    }
}

fun main() {
    val url = "https://www.tiktok.com/@scout2015/video/6798122930527931653"
    val metadataJson = MmDlp.extract(url)
    metadataJson?.let {
        println(it)
    }
}
```

### C#

```csharp
using System;
using System.Runtime.InteropServices;

public static class MmDlp
{
    [DllImport("mm_dlp", EntryPoint = "extract_metadata")]
    private static extern IntPtr ExtractMetadata(string url);

    [DllImport("mm_dlp", EntryPoint = "free_string")]
    private static extern void FreeString(IntPtr ptr);

    public static string Extract(string url)
    {
        IntPtr resultPtr = ExtractMetadata(url);
        if (resultPtr != IntPtr.Zero)
        {
            string resultJson = Marshal.PtrToStringAnsi(resultPtr);
            FreeString(resultPtr);
            return resultJson;
        }
        return null;
    }

    public static void Main(string[] args)
    {
        string url = "https://soundcloud.com/official-rick-astley/never-gonna-give-you-up-4";
        string metadataJson = Extract(url);
        if (metadataJson != null)
        {
            Console.WriteLine(metadataJson);
        }
    }
}
```

### Dart

```dart
import 'dart:ffi';
import 'package:ffi/ffi.dart';

// Define the function signatures from the Rust library
typedef ExtractMetadataC = Pointer<Utf8> Function(Pointer<Utf8> url);
typedef ExtractMetadataDart = Pointer<Utf8> Function(Pointer<Utf8> url);

typedef FreeStringC = Void Function(Pointer<Utf8> ptr);
typedef FreeStringDart = void Function(Pointer<Utf8> ptr);

void main() {
  final dylib = DynamicLibrary.open('libmm_dlp.so');

  final extractMetadata = dylib.lookupFunction<ExtractMetadataC, ExtractMetadataDart>('extract_metadata');
  final freeString = dylib.lookupFunction<FreeStringC, FreeStringDart>('free_string');

  final url = 'https://vimeo.com/123456789';
  final urlPtr = url.toNativeUtf8();
  
  final resultPtr = extractMetadata(urlPtr);
  
  if (resultPtr != nullptr) {
    final resultJson = resultPtr.toDartString();
    print(resultJson);
    freeString(resultPtr);
  }

  malloc.free(urlPtr);
}
```

### C

```c
#include <stdio.h>
#include <stdlib.h>

// Declare the functions from the Rust library
char* extract_metadata(const char* url);
void free_string(char* ptr);

int main() {
    const char* url = "https://www.twitch.tv/videos/123456789";
    char* result_json = extract_metadata(url);

    if (result_json != NULL) {
        printf("Extracted: %s\n", result_json);
        // IMPORTANT: Free the memory allocated by Rust
        free_string(result_json);
    } else {
        printf("Failed to extract metadata.\n");
    }

    return 0;
}
```
