using System;
using System.Runtime.InteropServices;

public class MmDlp
{
    [DllImport("mm_dlp", CallingConvention = CallingConvention.Cdecl)]
    public static extern ulong add(ulong left, ulong right);
}