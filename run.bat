REM serialitm can be found here: https://github.com/ninjasource/serialitm.git
cd ..\serialitm
start "serialitm" cmd.exe /k "cargo run com3"

cd ..\led-display-websocket-demo\led-display-hardware-ssl
start "openocd" cmd.exe /k "openocd"
start "demo hardware" cmd.exe /k "cargo run"
