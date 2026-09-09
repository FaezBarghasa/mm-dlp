import init, { add } from '../../pkg/mm_dlp.js'; // Assumes output generated from wasm-pack

$(document).ready(async function() {
    // Initialize the WebAssembly module
    await init();
    
    $('#addButton').click(function() {
        const left = BigInt($('#leftInput').val());
        const right = BigInt($('#rightInput').val());
        $('#result').text(add(left, right).toString());
    });
});