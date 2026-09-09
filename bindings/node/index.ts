import * as ffi from 'ffi-napi';
import * as path from 'path';

const lib = ffi.Library(path.join(__dirname, 'libmm_dlp'), {
    'add': [ 'uint64', [ 'uint64', 'uint64' ] ]
});

export function add(left: number, right: number): number {
    return lib.add(left, right);
}