# usage: hover.py <ox> <oy> <seconds>: sweep the pointer over the table rows at 120 Hz
import ctypes, sys, time
x11 = ctypes.CDLL("libX11.so.6"); xtst = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
d = x11.XOpenDisplay(None)
ox, oy, secs = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3])
end = time.time() + secs; i = 0
while time.time() < end:
    y = 70 + (i * 3) % 160
    x = 50 + (i * 7) % 600
    xtst.XTestFakeMotionEvent(ctypes.c_void_p(d), 0, ox + x, oy + y, 0)
    x11.XFlush(ctypes.c_void_p(d))
    i += 1
    time.sleep(1 / 120)
