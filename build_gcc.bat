@echo off
setlocal

set ROOT=%~dp0
set OUTDIR=%ROOT%build
set COMMON_INC=%ROOT%common\include

if not exist "%OUTDIR%" mkdir "%OUTDIR%"

gcc -shared -o "%OUTDIR%\\mai2io.dll" ^
    "%ROOT%mai2io\\src\\mai2io.c" ^
    "%ROOT%mai2io\\src\\affine_io.c" ^
    "%ROOT%common\\src\\affine_serial.c" ^
    "%ROOT%common\\src\\serial_win.c" ^
    "%ROOT%common\\src\\dprintf.c" ^
    "%ROOT%mai2io\\mai2io.def" ^
    -I"%COMMON_INC%" ^
    -I"%ROOT%mai2io\\include" ^
    -lsetupapi

gcc -shared -o "%OUTDIR%\\chuniio_affine.dll" ^
    "%ROOT%chuniio\\src\\chuniio.c" ^
    "%ROOT%common\\src\\affine_serial.c" ^
    "%ROOT%common\\src\\serial_win.c" ^
    "%ROOT%common\\src\\dprintf.c" ^
    "%ROOT%chuniio\\chuniio.def" ^
    -I"%COMMON_INC%" ^
    -I"%ROOT%chuniio\\include" ^
    -lsetupapi

gcc -shared -o "%OUTDIR%\\mercuryio_affine.dll" ^
    "%ROOT%mercuryio\\src\\mercuryio.c" ^
    "%ROOT%common\\src\\affine_serial.c" ^
    "%ROOT%common\\src\\serial_win.c" ^
    "%ROOT%common\\src\\dprintf.c" ^
    "%ROOT%mercuryio\\mercuryio.def" ^
    -I"%COMMON_INC%" ^
    -I"%ROOT%mercuryio\\include" ^
    -lsetupapi

gcc -shared -o "%OUTDIR%\\aimeio_affine.dll" ^
    "%ROOT%aimeio\\src\\aimeio.c" ^
    "%ROOT%common\\src\\affine_serial.c" ^
    "%ROOT%common\\src\\serial_win.c" ^
    "%ROOT%common\\src\\dprintf.c" ^
    "%ROOT%aimeio\\aimeio.def" ^
    -I"%COMMON_INC%" ^
    -I"%ROOT%aimeio\\include" ^
    -lsetupapi

endlocal
