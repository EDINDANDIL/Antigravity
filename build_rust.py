import os
import re
import subprocess
import sys
import time

if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
if hasattr(sys.stderr, 'reconfigure'):
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

# Must match src\auth.rs (LICENSE_VERSION_SEP): the license key is derived from
# base_secret + SEP + version, so keys are unique per release.
LICENSE_VERSION_SEP = "::"


def read_base_secret(auth_rs_path):
    """Reads the committed base secret straight from src\\auth.rs, so the source
    is the single source of truth for both the binary and the key generator."""
    with open(auth_rs_path, 'r', encoding='utf-8') as f:
        src = f.read()
    m = re.search(r'const\s+LICENSE_BASE_SECRET\s*:\s*&str\s*=\s*"([^"]*)"', src)
    if not m:
        raise RuntimeError("LICENSE_BASE_SECRET not found in src/auth.rs")
    return m.group(1)


def main():
    # Set correct working directory to where this script is located
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    print("[INFO] Starting build process...")

    VERSION = "2.5.0"
    version = VERSION
    # env!("CARGO_PKG_VERSION") only sees MAJOR.MINOR.PATCH, so the key salt uses
    # the same trimmed value the binary will compile with.
    cargo_version = ".".join(version.split(".")[:3])
    print(f"[INFO] Build version: {version}")

    # `.secrets.json` is now purely an owner marker: when present, this machine
    # is the author's, so the key generator is produced and fresh keys printed.
    # A clone from GitHub has no such file, so it builds a working binary but
    # generates NO keys - users must fetch a free key from t.me/nova_txt.
    is_owner = os.path.exists(".secrets.json")

    main_rs_path = r"src\main.rs"
    auth_rs_path = r"src\auth.rs"

    # The secret is committed in the sources; the key generator derives from the
    # exact same base + version salt, so its keys always match this build.
    base_secret = read_base_secret(auth_rs_path)
    key_secret_phrase = f"{base_secret}{LICENSE_VERSION_SEP}{cargo_version}"

    keygen_code = """import sys
import os
import subprocess
import traceback

SECRET_PHRASE = """ + repr(key_secret_phrase) + """

def copy_to_clipboard(text):
    try:
        import ctypes
        if not ctypes.windll.user32.OpenClipboard(None):
            return False
        ctypes.windll.user32.EmptyClipboard()
        hCd = ctypes.windll.kernel32.GlobalAlloc(2, len(text) + 1)
        if not hCd:
            ctypes.windll.user32.CloseClipboard()
            return False
        pCd = ctypes.windll.kernel32.GlobalLock(hCd)
        if not pCd:
            ctypes.windll.user32.CloseClipboard()
            return False
        ctypes.cdll.msvcrt.strcpy(ctypes.c_char_p(pCd), text.encode('ascii'))
        ctypes.windll.kernel32.GlobalUnlock(hCd)
        ctypes.windll.user32.SetClipboardData(1, hCd)
        ctypes.windll.user32.CloseClipboard()
        return True
    except Exception:
        try:
            subprocess.run(
                ["powershell", "-NoProfile", "-Command", f"Set-Clipboard -Value '{text}'"],
                shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
            )
            return True
        except:
            return False

def generate_key():
    import hashlib
    import random
    import string
    chars = string.ascii_uppercase + string.digits
    nonce = ''.join(random.choices(chars, k=12))
    data = f"{nonce}{SECRET_PHRASE}".encode('utf-8')
    signature = hashlib.sha256(data).hexdigest().upper()[:12]
    return f"{nonce}{signature}"

def run_gui_tk():
    import tkinter as tk
    from tkinter import messagebox

    def copy_selected():
        try:
            selected_idx = listbox.curselection()
            if not selected_idx:
                messagebox.showwarning("Warning", "Please select a key first!")
                return
            selected_key = listbox.get(selected_idx[0])
            root.clipboard_clear()
            root.clipboard_append(selected_key)
            root.update()
            status_label.config(text="Selected key copied!", fg="#a6e3a1")
        except Exception as e:
            messagebox.showerror("Error", str(e))

    def copy_all():
        try:
            all_keys = listbox.get(0, tk.END)
            keys_str = "\\n".join(all_keys)
            root.clipboard_clear()
            root.clipboard_append(keys_str)
            root.update()
            status_label.config(text="All 5 keys copied!", fg="#a6e3a1")
        except Exception as e:
            messagebox.showerror("Error", str(e))

    global root, listbox, status_label
    root = tk.Tk()
    root.title("Antigravity Keygen v""" + version + """")
    root.geometry("450x420")
    root.configure(bg="#1e1e2e")
    root.resizable(False, False)
    
    title_lbl = tk.Label(root, text="Antigravity Key Generator", bg="#1e1e2e", fg="#89b4fa", font=("Segoe UI", 16, "bold"))
    title_lbl.pack(pady=15)
    
    frame = tk.Frame(root, bg="#1e1e2e")
    frame.pack(pady=10)
    
    listbox = tk.Listbox(
        frame, 
        bg="#181825", 
        fg="#a6e3a1", 
        font=("Consolas", 14, "bold"), 
        width=28, 
        height=5, 
        bd=0, 
        highlightbackground="#313244", 
        highlightcolor="#89b4fa", 
        highlightthickness=2,
        selectbackground="#45475a",
        selectforeground="#ffffff",
        activestyle="none"
    )
    listbox.pack(side=tk.LEFT, fill=tk.BOTH)
    
    for _ in range(5):
        listbox.insert(tk.END, generate_key())
    
    listbox.selection_set(0)
    
    status_label = tk.Label(root, text="Select a key to copy", bg="#1e1e2e", fg="#a6adc8", font=("Segoe UI", 10))
    status_label.pack(pady=5)
    
    btn_style = {
        "bg": "#313244",
        "fg": "#cdd6f4",
        "activebackground": "#45475a",
        "activeforeground": "#ffffff",
        "font": ("Segoe UI", 11, "bold"),
        "bd": 0,
        "height": 2,
        "width": 18,
        "cursor": "hand2"
    }
    
    btn_frame = tk.Frame(root, bg="#1e1e2e")
    btn_frame.pack(pady=15)
    
    btn_copy_sel = tk.Button(btn_frame, text="Copy Selected", command=copy_selected, **btn_style)
    btn_copy_sel.pack(side=tk.LEFT, padx=10)
    
    btn_copy_all = tk.Button(btn_frame, text="Copy All", command=copy_all, **btn_style)
    btn_copy_all.pack(side=tk.LEFT, padx=10)
    
    def on_enter(e):
        e.widget.config(bg="#45475a")
    def on_leave(e):
        e.widget.config(bg="#313244")
    
    btn_copy_sel.bind("<Enter>", on_enter)
    btn_copy_sel.bind("<Leave>", on_leave)
    btn_copy_all.bind("<Enter>", on_enter)
    btn_copy_all.bind("<Leave>", on_leave)
    
    root.mainloop()

def run_gui_dpg():
    import dearpygui.dearpygui as dpg

    keys = [generate_key() for _ in range(5)]
    selected_idx = 0

    dpg.create_context()
    dpg.create_viewport(title='Antigravity Keygen v""" + version + """', width=450, height=420, resizable=False)
    dpg.setup_dearpygui()

    def copy_selected_callback():
        nonlocal selected_idx
        text = keys[selected_idx]
        if copy_to_clipboard(text):
            dpg.set_value(status_text, "Selected key copied!")
        else:
            dpg.set_value(status_text, "Failed to copy key.")

    def copy_all_callback():
        text = "\\n".join(keys)
        if copy_to_clipboard(text):
            dpg.set_value(status_text, "All 5 keys copied!")
        else:
            dpg.set_value(status_text, "Failed to copy keys.")

    def listbox_callback(sender, app_data):
        nonlocal selected_idx
        selected_idx = keys.index(app_data)

    with dpg.window(label="Main Window", width=450, height=420, no_title_bar=True, no_move=True, no_resize=True):
        dpg.add_spacer(height=15)
        dpg.add_text("Antigravity Key Generator", color=[137, 180, 250])
        dpg.add_spacer(height=15)
        
        dpg.add_listbox(items=keys, callback=listbox_callback, width=410, num_items=5)
        dpg.add_spacer(height=10)
        
        status_text = dpg.add_text("Select a key to copy", color=[166, 173, 200])
        dpg.add_spacer(height=15)
        
        with dpg.group(horizontal=True):
            dpg.add_button(label="Copy Selected", callback=copy_selected_callback, width=195, height=40)
            dpg.add_button(label="Copy All", callback=copy_all_callback, width=195, height=40)

    with dpg.theme() as global_theme:
        with dpg.theme_component(dpg.mvAll):
            dpg.add_theme_color(dpg.mvThemeCol_WindowBg, [30, 30, 46])
            dpg.add_theme_color(dpg.mvThemeCol_Button, [49, 50, 68])
            dpg.add_theme_color(dpg.mvThemeCol_ButtonHover, [69, 71, 90])
            dpg.add_theme_color(dpg.mvThemeCol_ButtonActive, [137, 180, 250])
            dpg.add_theme_color(dpg.mvThemeCol_FrameBg, [24, 24, 37])
            dpg.add_theme_color(dpg.mvThemeCol_Text, [205, 214, 244])

    dpg.bind_theme(global_theme)
    dpg.show_viewport()
    dpg.start_dearpygui()
    dpg.destroy_context()

def run_console():
    os.system('color 0A' if os.name == 'nt' else 'clear')
    os.system('cls' if os.name == 'nt' else 'clear')
    print("Ключи для v""" + version + """\\n")
    for _ in range(5):
        print(generate_key())
    
    print()
    input("Press Enter to exit...")

def main():
    try:
        run_gui_tk()
        return
    except ImportError:
        try:
            import dearpygui.dearpygui
            run_gui_dpg()
            return
        except ImportError:
            print("GUI module 'tkinter' is missing. Installing 'dearpygui' as alternative...")
            try:
                subprocess.check_call([sys.executable, "-m", "pip", "install", "dearpygui"])
                import dearpygui.dearpygui
                run_gui_dpg()
                return
            except Exception:
                pass
    except Exception as e:
        pass

    run_console()

if __name__ == "__main__":
    main()
"""

    dist_keygen_path = "dist_keygen.py"

    try:
        # 1. Sync the version in Cargo.toml so env!("CARGO_PKG_VERSION") - which
        #    both the binary and the key salt use - matches this release.
        with open("Cargo.toml", 'r', encoding='utf-8') as f:
            cargo_content = f.read()
        cargo_content = re.sub(r'^version\s*=\s*"[^"]*"', f'version = "{cargo_version}"', cargo_content, flags=re.MULTILINE)
        with open("Cargo.toml", 'w', encoding='utf-8') as f:
            f.write(cargo_content)

        # 2. Owner-only: (re)generate the key generator for THIS version. Never
        #    created on a plain clone from GitHub - those builds ship no keys.
        if is_owner:
            with open(dist_keygen_path, 'w', encoding='utf-8') as f:
                f.write(keygen_code)

        # 3. Build the release binary (only the main target). No source edits are
        #    made, so there is nothing sensitive that could be left behind.
        print("[INFO] Запуск компиляции (Release mode)...")
        subprocess.check_call(["cargo", "build", "--release", "--bin", "ag_unlocker"])

        import shutil
        os.makedirs("release", exist_ok=True)
        out_path = os.path.abspath(os.path.join("release", f"AG_{version}.exe"))
        # Terminate any running instance of the previous exe to avoid PermissionError
        subprocess.run(["taskkill", "/F", "/IM", f"AG_{version}.exe"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(0.5)
        # Move (not copy) so target/release/ag_unlocker.exe is not left behind.
        if os.path.exists(out_path):
            os.remove(out_path)
        shutil.move(r"target\release\ag_unlocker.exe", out_path)

        # Compress with UPX if available.
        print("[INFO] Сжатие исполняемого файла с помощью UPX...")
        try:
            subprocess.run(["upx", "--best", "--lzma", out_path], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            print("[INFO] Исполняемый файл успешно сжат с помощью UPX.")
        except Exception as e:
            print(f"[WARNING] Не удалось сжать файл с помощью UPX: {e}")

        print("\n[УСПЕХ] Сборка завершена!")
        print(f"Ваш исполняемый файл: {out_path}")

        if is_owner:
            print(f"Ваш генератор ключей для этой версии: {dist_keygen_path}")
            print(f"\n[i] Ключи привязаны к версии {cargo_version}: на следующем релизе")
            print("    (после смены VERSION здесь) они перестанут подходить, и людям")
            print("    понадобится новый ключ из t.me/nova_txt.")
            # Auto-generate some keys for convenience.
            print(f"\n5 ключей для версии {cargo_version}:")
            subprocess.check_call(["python", "-c", "import dist_keygen; [print(dist_keygen.generate_key()) for _ in range(5)]"])
        else:
            print("\nДля работы необходим ключ - получить его можно бесплатно в группе t.me/nova_txt")

    except subprocess.CalledProcessError as e:
        print(f"\n[ОШИБКА] Сборка завершилась с ошибкой: {e}")
    except Exception as e:
        print(f"\n[ОШИБКА] Непредвиденная ошибка сборки: {e}")
    finally:
        # Clean the target folder to save space and keep the repo clean.
        print("\n[INFO] Очистка временных файлов сборки (cargo clean)...")
        try:
            subprocess.run(["cargo", "clean"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except Exception:
            pass

if __name__ == "__main__":
    main()
