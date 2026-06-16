import os
import shutil

def rename_installers():
    root_dir = "downloaded-artifacts"
    print("=== Start Renaming Stripped Installers ===")
    if not os.path.exists(root_dir):
        print(f"Directory {root_dir} does not exist!")
        return

    for root, dirs, files in os.walk(root_dir):
        for f in files:
            if f.startswith("_"):
                old = os.path.join(root, f)
                new = os.path.join(root, "顾问工作台" + f)
                print(f"Renaming: {old} -> {new}")
                shutil.move(old, new)
    print("=== End Renaming Stripped Installers ===")

if __name__ == "__main__":
    rename_installers()
