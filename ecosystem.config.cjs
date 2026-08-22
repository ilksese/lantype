module.exports = {
  apps: [
    {
      name: 'lantype-dev',
      script: 'npm',
      args: 'run dev',
      cwd: __dirname,
      watch: false,
      autorestart: true,
    },
  ],
}
