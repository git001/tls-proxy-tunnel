local executableName = 'fourth';
local build_image = 'img.kie.rs/jjkiers/rust-cross:rust1.70-zig';

local archs = [
  { target: 'aarch64-unknown-linux-musl', short: 'arm64-musl' },
  { target: 'x86_64-pc-windows-gnu', short: 'windows' },
  { target: 'x86_64-unknown-linux-musl', short: 'amd64-musl' },
];

local getStepName(arch) = 'Build for ' + arch.short;

local builtExecutableName(arch) = executableName + if std.length(std.findSubstr(arch.short, 'windows')) > 0 then '.exe' else '';
local targetExecutableName(arch) = executableName + '-' + arch.target + if std.length(std.findSubstr(arch.short, 'windows')) > 0 then '.exe' else '';

local getVolumeName(arch) = 'target-' + arch.target;
local getLocalVolumes(arch) = [
  {
    name: getVolumeName(arch),
    temp: {},
  }
  for arch in archs
];

local add_build_steps() = [
  {
    name: getStepName(arch),
    image: build_image,
    commands: [
      'echo Hello World from Jsonnet on ' + arch.target + '!',
      'cargo zigbuild --release --target ' + arch.target,
      'cp target/' + arch.target + '/release/' + builtExecutableName(arch) + ' artifacts/' + targetExecutableName(arch),
      'rm -rf target/' + arch.target + '/release/*',
    ],
    depends_on: ['Prepare'],
    volumes: [{
      name: getVolumeName(arch),
      path: '/drone/src/target',
    }],
  }
  for arch in archs
];

{
  kind: 'pipeline',
  type: 'docker',
  name: 'default',
  platform: {
    arch: 'amd64',
  },
  steps:
    [{
      name: 'Prepare',
      image: build_image,
      commands: [
        'mkdir artifacts',
        'echo Using image: ' + build_image,
        'cargo --version',
        'rustc --version',
      ],
    }] +
    add_build_steps() +
    [
      {
        name: 'Show built artifacts',
        image: build_image,
        commands: [
          'ls -lah artifacts',
        ],
        depends_on: [getStepName(a) for a in archs],
      },
      {
        name: 'Create release on gitea',
        image: 'plugins/gitea-release',
        settings: {
          api_key: {
            from_secret: 'gitea_token',
          },
          base_url: 'https://code.kiers.eu',
          files: 'artifacts/*',
          checksum: 'sha256',
        },
        when: {
          event: ['tag', 'promote'],
        },
        depends_on: ['Show built artifacts'],
      },
    ],

  volumes: getLocalVolumes(archs),

  image_pull_secrets: ['docker_private_repo'],
}
