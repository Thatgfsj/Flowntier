# -*- mode: python ; coding: utf-8 -*-
import os
from PyInstaller.utils.hooks import collect_submodules

# Get absolute paths
spec_dir = os.path.dirname(os.path.abspath(SPEC))
runtime_src = os.path.join(spec_dir, '..', '..', 'runtime', 'src')
app_runtime_src = os.path.join(spec_dir, 'src')

hiddenimports = ['aco_runtime_lib', 'aco_runtime']
hiddenimports += collect_submodules('aco_runtime_lib')
hiddenimports += collect_submodules('aco_runtime')
# Explicitly add worker_v2 in case collect_submodules misses it
hiddenimports += ['aco_runtime_lib.agents.worker_v2']
# Windows named-pipe transport (v0.2.5+). pywin32 ships native DLLs in
# pywin32_system32 that PyInstaller needs to copy next to the EXE.
hiddenimports += ['win32pipe', 'win32file', 'win32api', 'pywintypes', 'pywin32_system32']

a = Analysis(
    [os.path.join(runtime_src, 'aco_runtime_lib', '__main__.py')],
    pathex=[runtime_src, app_runtime_src],
    binaries=[],
    datas=[],
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='aco_runtime',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
