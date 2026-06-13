import 'dart:ffi' as ffi;
import 'dart:io' show Platform;

typedef AddFunc = ffi.Uint64 Function(ffi.Uint64 left, ffi.Uint64 right);
typedef Add = int Function(int left, int right);

class MmDlp {
  late final ffi.DynamicLibrary _lib;
  late final Add add;

  MmDlp() {
    if (Platform.isLinux || Platform.isAndroid) {
      _lib = ffi.DynamicLibrary.open('libmm_dlp.so');
    } else if (Platform.isMacOS || Platform.isIOS) {
      _lib = ffi.DynamicLibrary.open('libmm_dlp.dylib');
    } else if (Platform.isWindows) {
      _lib = ffi.DynamicLibrary.open('mm_dlp.dll');
    } else {
      throw UnsupportedError('Unknown platform');
    }

    add = _lib.lookupFunction<AddFunc, Add>('add');
  }
}