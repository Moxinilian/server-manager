# Server setup guide

This guide will help you set up and use a VPS for Minecraft servers with server-manager.

## Using an installed server

An already set-up server should be pretty autonomous. But if you need to access the files or the console of the server, the procedure is described here.

### Navigating the system over SSH

In order to connect to the remote machine, first connect as the provided admin user (root user connection is disabled).

Once this is achieved, change your user to the `minecraft` user and move to its home directory.

```
$ sudo su minecraft
```

```
$ cd ~
```

The server files should be somewhere in the home directory.

### Accessing the console

The server is hosted within a tmux session you can access like so:

```
$ tmux attach -t minecraft
```

There might be multiple panes if multiple servers are configured.

Do not terminate the process to exit the session (using for example Ctrl+C) as this will stop the server manager. Instead, **pressing Ctrl+B then pressing D** should be used to exit the console.

### Transfering files

Once you have located a file you would like to download or upload to the server, use SCP from your **local machine** to transfer the files. On Linux, it can be done using the following command for download:

```
$ scp remote:/path/in/remote path/in/local
```

Replace `remote` with the actual remote address, for example `admin@xxx.xxx.xxx.xxx`. It should be the same as what you use for SSH. Swap the parameters for upload:

```
$ scp path/in/local remote:/path/in/remote
```

On Windows, you can use WinSCP instead.

Note that after uploading, the files will be owned by the admin user. To make them owned by the minecraft user, use chown in SSH as the admin user.

```
$ sudo chown minecraft file/to/change
```

Alternatively, you can recursively chown the home directory of the minecraft user after the upload to make sure it still owns all the files in its home directory.

```
$ sudo chown -R minecraft /home/minecraft
```

### Restoring a backup

To restore the last backup from backup data stored either on the VPS or remotely, use duplicity's backup restore feature.

```
$ duplicity url://to/the/backup/data path/wher/to/restore
```

The backup data should be provided as a URL:

- If it is already stored locally, use the file URL scheme. For example, `file:///path/to/backup`.
- If it is stored remotely, use the URL associated to your remote storage solution. Note that most cloud storage providers charge for the download of data, thus prefer to restore from local data when possible.

To restore an older backup, use the `-t` argument in duplicity.

```
$ duplicity -t 2W url://to/the/backup/data path/wher/to/restore
```

For example, the command above will restore the newest data before the date 2 weeks ago (`2W`). Other possible time descriptors are `2D` (2 days), `5h` (5 hours), etc. You can also use Unix timestamps. See [duplicity's manual page](http://duplicity.nongnu.org/vers7/duplicity.1.html#sect8) for all possible time formats.

## Setting up a new server

### Creating the new admin user

Extremely important: pick a **very strong** password for your root user.

Once you connect with SSH into your server as root, the first thing to do is to create a separate sudoer to be able to block remote access to root. This notably helps avoid being targetted by brute forcers.

Let's create an `admin` user (feel free to pick a different, more exotic name). Give it a **very strong** password as well.

```
# adduser admin
```

Answer the questions the tool will ask you. Once done, a new user and a group will be created, along with a home directory.

Then, let's make it a sudoer.

```
# usermod -aG sudo admin
```

Then, let's disable SSH root login. Open `/etc/ssh/sshd_config`.

```
# nano /etc/ssh/sshd_config
```

Add the following line to the file:

```
PermitRootLogin no
```

Restart the SSH service for changes to take effect.

```
# systemctl restart sshd
```

You can now disconnect from SSH. Try to reconnect again to make sure it does not work. Connect back with the new `admin` user.

### Installing dependencies

Adapt the following command to your package manager.

```
sudo apt install tmux git curl duplicity rclone openjdk-17-jre-headless
```

Install the Rust compiler.

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Setting up the minecraft user

Now, let's create an user for the minecraft server. Again, pick strong passwords.

```
$ sudo adduser admin
```

You should add your `admin` user to the `minecraft` group to make it easier to manage the files of `minecraft` from the admin account.

```
$ usermod -aG minecraft admusr
```

We won't connect as this user via SSH either, so let's block its SSH access.

```
$ sudo nano /etc/ssh/sshd_config
```

Add the following line (or only add the user name if it already exists):

```
DenyUsers minecraft
```

Again, restart the SSH service.

```
$ sudo systemctl restart sshd
```

Next, switch to the `minecraft` user.

```
$ sudo su minecraft
```

Switch to the home directory of `minecraft` (or anywhere within it).

```
$ cd ~
```

Create a folder for your new Minecraft server.

```
$ mkdir my-minecraft-server
```

### Building server-manager

Clone server-manager.

```
$ git clone https://github.com/Moxinilian/server-manager
```

```
$ cd server-manager
```

Build it.

```
$ cargo build --release
```

Copy the built binary to somewhere easier to use, for example the home directory.

```
$ cp target/release/server-manager ~/server-manager
```

### Setting up a basic server

Go to your Minecraft server folder.

```
cd ~/my-minecraft-server
```

Download the Minecraft server JAR you want. Here, we download a JAR for Minecraft 1.18.

```
curl https://launcher.mojang.com/v1/objects/9a03d2c4ec2c737ce9d17a43d3774cdc0ea21030/server.jar --output minecraft-server.jar
```

Start server-manager once to generate a dummy configuration file.

```
$ ../server-manager
```

Edit the server-manager config `server-manager.ron` to your liking.

Once done, start the manager again. Accept the Minecraft EULA. Restart the server-manager if it gave up on restarting the server.

The server should now have generated a config file. Stop server-manager by using `Ctrl+C` (or any other way to send it a `SIGINT` signal).

### Enabling RCON

Open `server.properties` and configure it to your liking. Specifically, set the following properties:

- `enable-rcon` to `true`
- `rcon.password` to a **very strong** password. You can use the one server-manager generated in `server-manager.ron`. Update the password in `server-manager.ron` if you use a different one.
- Make sure `rcon.port` and the `rcon_port` value in `server-manager.ron` match.

To communicate with the server, server-manager will use RCON. As such, it is much better security-wise to restrict RCON to local access only. One can achieve this using the following commands (assuming the RCON port you use is 25575). Note that **they must be executed in this order**.

```
$ iptables -A INPUT -p tcp -s localhost --dport 25575 -j ACCEPT
```
```
$ iptables -A INPUT -p tcp --dport 25575 -j DROP
```

Order matters here because they respectively add the following rules:

- If a connection comes from localhost on port 25575, accept it.
- If a connection comes from any IP on port 25575, deny it.

As rules are treated in order, this allows localhost being accepted even if there is a drop-all rule afterwards.

### Setting up a persistent server

We should now start the server-manager again. But if you do it by just calling the server-manager program, it will close as soon as you terminate your SSH session. Instead, let's create a persistent tmux session for it.

```
$ tmux new-session -d -s minecraft '../server-manager'
```

This command created a tmux session named `minecraft`. Check it exists by running

```
$ tmux list-sessions
```

To attach to it in order to access your server's console, run

```
$ tmux attach -t minecraft
```

Be careful: if you terminate the process by doing Ctrl+C, this will close your server. Detaching from the session should instead be done by **pressing Ctrl+B then pressing D**. After this, you can safely terminate your SSH session: the server will still be running in the background.

Your server should now be operational. But if you used the default configuration, it currently only does local backups, and if your VPS restarts, it will not restart with it.

### Making the server restart after a VPS shutdown

It is useful to automate the restart of the server manager in case of an unplanned shutdown of the host machine.

To do this, we will use cron. As the `minecraft` user, open the cron table.

```
$ crontab -e
```

Then add the following line:

```
@reboot tmux new-session -d -s minecraft 'cd ~/path/to/server/folder && ~/path/to/server-manager'
```

Don't forget to update the paths to your server manager and server folder according to your previous configuration.

Restart the VPS to ensure the addition works properly.

### Set up remote backups

By default, server-manager sets up local backups in `server-manager.ron`.

```ron
backups: Some((
        // where to store backups relative to server-manager working directory
        backup_folder: "./backups", 

        // what to backup relative to server folder
        world_folder: "world",

        // how many hours between incremental backups
        incremental_freq_hours: 1,
        
        // how many hours between full backups in place of the next incremental backup
        full_backup_every: 336, 

        // maximum amount of full backups to keep
        // oldest full backups and dependent incremental backups will be deleted
        // when this threshold is passed
        keep_full_backup: 2, 

        // whether backups should be synced remotely
        // and if so, where (more on that later)
        rclone_path: None,

        // whether to make the Minecraft server flush all chunks
        // on save, risking freezes on backup but guaranteeing data
        // integrity to a ridiculous level
        flush_on_save: true,

        // whether to silent backup messages in the Minecraft chat
        silent: false,
    ))
```

In order to have server-manager also sync backup data to an offsite location, you must first pick a remote storage provider. You can roll your own solution but I recommend Backblaze B2 as they are very inexpensive, offer 10GB hosting for free, are compatible with all the tools used here and globally offer an easy to use experience. 

Configure your remote location with rclone:

```
$ rclone config
```

Select "New remote" then follow the instructions specific to your remote provider. If you are using Backblaze B2, you can follow [this tutorial](https://help.backblaze.com/hc/en-us/articles/1260804565710-How-to-use-rclone-with-Backblaze-B2-Cloud-Storage).

Once your remote provider is configured, set the remote path to be used by server-manager in the config:

```ron
rclone_path: Some("my_remote:path/to/backup"),
```

Restart server-manager for changes to take effect.

### Set up incident mail reports

By default, server-manager does not configure mail reports. Replace the `mailing` value in `server-manager.ron` with:

```ron
    mailing: Some((
        contacts: ["Alice <alice@example.cm>", "Bob <bob@example.com>"],
        smtp_server: "<your mail server host>",
        sender: "Minecraft Mail Report <mail.report@example.com>",
        username: "<your mail server username>",
        password: "<your mail server password>",
    )),
```

filling it with corresponding data.

Note that if you are using Gmail as a mail server, you need to use application passwords instead of your actual account password in order for this to work.

Restart server-manager for changes to take effect.