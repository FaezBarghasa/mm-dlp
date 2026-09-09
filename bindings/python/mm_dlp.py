import ctypes
import os

# Load the shared library (change extension for Windows/macOS if necessary)
_lib = ctypes.cdll.LoadLibrary(os.path.abspath("libmm_dlp.so"))

_lib.add.argtypes = [ctypes.c_uint64, ctypes.c_uint64]
_lib.add.restype = ctypes.c_uint64

def add(left: int, right: int) -> int:
    return _lib.add(left, right)