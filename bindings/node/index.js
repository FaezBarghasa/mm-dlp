const ffi = require('ffi-napi');
const path = require('path');

const lib = ffi.Library(path.join(__dirname, 'libmm_dlp'), {
    'add': [ 'uint64', [ 'uint64', 'uint64' ] ]
});

function add(left, right) {
    return lib.add(left, right);
}

module.exports = { add };