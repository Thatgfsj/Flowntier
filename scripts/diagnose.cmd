@echo off
REM BUG-FRONTEND-RT-??: diagnosis script for the chairman.
REM Collects: running processes, env vars, install dir state,
REM NSIS log, %APPDATA% state, and saves to
REM C:\Users\thatg\flowntier-diagnose.txt
REM Please attach this file when reporting issues.

set OUTPUT=%USERPROFILE%\flowntier-diagnose.txt

echo ======================================
echo Flowntier Diagnosis Report
echo Generated: %date% %time%
echo ======================================
echo.

echo --- 1. Flowntier processes (tasklist) ---
tasklist /FI "IMAGENAME eq flowntier.exe" 2>&1
tasklist /FI "IMAGENAME eq flowntier_runtime.exe" 2>&1
tasklist /FI "IMAGENAME eq pipe-server.exe" 2>&1
echo.

echo --- 2. Install directory state ---
if exist "O:\Flowntier" (
    echo O:\Flowntier\ exists:
    dir /B "O:\Flowntier\" 2>&1
) else (
    echo O:\Flowntier\ does NOT exist
)
if exist "C:\Program Files\Flowntier" (
    echo C:\Program Files\Flowntier\ exists:
    dir /B "C:\Program Files\Flowntier\" 2>&1
) else (
    echo C:\Program Files\Flowntier\ does NOT exist
)
echo.

echo --- 3. AppData (settings + keychain) ---
if exist "%APPDATA%\flowntier" (
    echo %%APPDATA%%\flowntier\ exists:
    dir /B "%APPDATA%\flowntier\" 2>&1
) else (
    echo %%APPDATA%%\flowntier\ does NOT exist
)
if exist "%LOCALAPPDATA%\flowntier" (
    echo %%LOCALAPPDATA%%\flowntier\ exists:
    dir /B "%LOCALAPPDATA%\flowntier\" 2>&1
) else (
    echo %%LOCALAPPDATA%%\flowntier\ does NOT exist
)
echo.

echo --- 4. Environment variables (API keys) ---
echo MINIMAX_API_KEY=%MINIMAX_API_KEY%
echo OPENAI_API_KEY=%OPENAI_API_KEY%
echo ANTHROPIC_API_KEY=%ANTHROPIC_API_KEY%
echo GOOGLE_API_KEY=%GOOGLE_API_KEY%
echo DEEPSEEK_API_KEY=%DEEPSEEK_API_KEY%
echo MOONSHOT_API_KEY=%MOONSHOT_API_KEY%
echo OPEN_BIGMODEL_API_KEY=%OPEN_BIGMODEL_API_KEY%
echo.

echo --- 5. Tauri NSIS log (if present) ---
if exist "%LOCALAPPDATA%\Temp\Flowntier*.log" (
    echo NSIS log found:
    dir "%LOCALAPPDATA%\Temp\Flowntier*.log" 2>&1
) else (
    echo No NSIS log in %%LOCALAPPDATA%%\Temp\
)
echo.

echo --- 6. Tauri crash log (if any) ---
if exist "%APPDATA%\flowntier\logs" (
    echo Logs directory exists:
    dir /B "%APPDATA%\flowntier\logs\" 2>&1
) else (
    echo %%APPDATA%%\flowntier\logs\ does NOT exist
)
echo.

echo ======================================
echo End of report
echo File saved to: %OUTPUT%
echo ======================================
echo Please attach this file when reporting issues.

REM Save to file
(
    echo ======================================
    echo Flowntier Diagnosis Report
    echo Generated: %date% %time%
    echo ======================================
    echo.
    echo --- 1. Flowntier processes (tasklist) ---
    tasklist /FI "IMAGENAME eq flowntier.exe" 2>&1
    tasklist /FI "IMAGENAME eq flowntier_runtime.exe" 2>&1
    tasklist /FI "IMAGENAME eq pipe-server.exe" 2>&1
    echo.
    echo --- 2. Install directory state ---
    if exist "O:\Flowntier" (
        echo O:\Flowntier\ exists:
        dir /B "O:\Flowntier\" 2>&1
    ) else (
        echo O:\Flowntier\ does NOT exist
    )
    if exist "C:\Program Files\Flowntier" (
        echo C:\Program Files\Flowntier\ exists:
        dir /B "C:\Program Files\Flowntier\" 2>&1
    ) else (
        echo C:\Program Files\Flowntier\ does NOT exist
    )
    echo.
    echo --- 3. AppData (settings + keychain) ---
    if exist "%APPDATA%\flowntier" (
        echo %%APPDATA%%\flowntier\ exists:
        dir /B "%APPDATA%\flowntier\" 2>&1
    ) else (
        echo %%APPDATA%%\flowntier\ does NOT exist
    )
    if exist "%LOCALAPPDATA%\flowntier" (
        echo %%LOCALAPPDATA%%\flowntier\ exists:
        dir /B "%LOCALAPPDATA%\flowntier\" 2>&1
    ) else (
        echo %%LOCALAPPDATA%%\flowntier\ does NOT exist
    )
    echo.
    echo --- 4. Environment variables (API keys) ---
    echo MINIMAX_API_KEY=%MINIMAX_API_KEY%
    echo OPENAI_API_KEY=%OPENAI_API_KEY%
    echo ANTHROPIC_API_KEY=%ANTHROPIC_API_KEY%
    echo GOOGLE_API_KEY=%GOOGLE_API_KEY%
    echo DEEPSEEK_API_KEY=%DEEPSEEK_API_KEY%
    echo MOONSHOT_API_KEY=%MOONSHOT_API_KEY%
    echo OPEN_BIGMODEL_API_KEY=%OPEN_BIGMODEL_API_KEY%
) > "%OUTPUT%" 2>&1
echo.
echo Report saved to: %OUTPUT%