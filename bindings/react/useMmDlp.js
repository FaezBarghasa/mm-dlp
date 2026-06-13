import { useEffect, useState } from 'react';
import init, { add } from '../../pkg/mm_dlp.js'; // Assumes output generated from wasm-pack

export function useMmDlp() {
    const [isReady, setIsReady] = useState(false);

    useEffect(() => {
        init().then(() => {
            setIsReady(true);
        });
    }, []);

    return { isReady, add };
}