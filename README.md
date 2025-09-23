# 🚀 Tauri Multi-Product Update Server

A self-hosted update server for **Tauri apps** that securely serves updates from **private GitHub releases**.  
It works as a proxy so your apps can update without exposing GitHub tokens.  

Configuration is **entirely environment-based**, making it perfect for **Docker deployments**.

---

## ✨ Features

- **Multi-Product Support** – Serve updates for multiple apps from one server.
- **Private Repositories** – Securely fetch release assets from private GitHub repos.
- **Dynamic Configuration** – No static config files; everything comes from environment variables.
- **Proxy Downloads** – Keeps tokens server-side and hides GitHub asset URLs.
- **Docker-Friendly** – Includes a `Dockerfile` for quick deployment.

---

## ⚙️ Setup

### 1. Environment Variables

Copy the provided `.env.SAMPLE` and adjust it for your products:

```dotenv
# --- Example Product Config ---
MYAPP_TOKEN=github_pat_xxxxxxxxxxxxxxxxxxxx
MYAPP_OWNER=your-github-username
MYAPP_REPO=my-awesome-app

# --- Another Product Example ---
ANOTHERAPP_TOKEN=github_pat_xxxxxxxxxxxxxxxxxxxx
ANOTHERAPP_OWNER=another-owner
ANOTHERAPP_REPO=another-repo

# --- Server Config (defaults: ADDRESS=0.0.0.0, PORT=8080) ---
ADDRESS=0.0.0.0
PORT=8080
HOSTNAME=https://updates.example.com
```

The `_TOKEN` variable (e.g., `MYAPP_TOKEN`) must be a [GitHub Personal Access Token (PAT)](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) with `read-only` permission for the **Contents** of your private repository to access its release assets.

On startup a map of the product configs is read from the .env. Add as many products as you wish.

---

### 2. Running the Server

**Locally**
```bash
cargo run
```

**With Docker**
```bash
docker build -t tauri-update-server .
docker run --rm -it --env-file ./.env -p 8080:8080 tauri-update-server
```

⚠️ Make sure the container port (`-p 8080:8080`) matches your `PORT` variable.

For production, consider hosting on Google Cloud Run, Fly.io, or Railway.
They all support deploying directly from a Dockerfile and make it easy to manage environment variables.

---

## 📦 Usage

### 1. GitHub Release Assets

When the `updater` plugin is active in your `tauri.conf.json`, Tauri's GitHub Action workflow automatically generates release assets with the correct naming convention. For more details, see the [official Tauri documentation](https://v2.tauri.app/distribute/pipelines/github).

- **Feature Channels**: To support channels like `beta`, prefix the asset filename (e.g., `BETA.my-app_1.2.0_x64.msi`). The stable channel uses files without a prefix.

### 2. Tauri Configuration

Refer to the [Updater plugin docs](https://v2.tauri.app/plugin/updater/) and the [official Tauri GitHub pipelines documentation](https://v2.tauri.app/distribute/pipelines/github) for full details.

In your `tauri.conf.json`, set the updater endpoints to your server:
```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://updates.example.com/myapp/stable/{{target}}/{{arch}}/{{current_version}}"
      ]
    }
  }
}
```

Replace the following in the URL:
- `updates.example.com` → your server’s `HOSTNAME`
- `myapp` → the product name (from your `.env` file)

