/**
 * English UI strings (source of truth for keys).
 *
 * Plain language only (AGENTS.md glossary): never mention hub, server, IP,
 * driver, capability, endpoint, token, relay or tunnel. The home is
 * "your home"; devices are called by their names.
 */
export const en = {
  app: {
    name: 'VESTA',
  },
  common: {
    cancel: 'Cancel',
    close: 'Close',
    notifications: 'Notifications',
    on: 'On',
    off: 'Off',
    percent: '{{value}}%',
  },
  nav: {
    label: 'Rooms and pages',
  },
  connection: {
    connecting: 'Connecting to your home…',
    lost: "Can't reach your home right now.",
    retrying: 'Trying again…',
    retryingIn: 'Trying again in {{seconds}} s',
    retryNow: 'Try now',
    connected: 'Connected to your home',
    waitingTitle: 'Waiting for your home',
    waitingBody: 'This screen will show your home as soon as it connects.',
  },
  display: {
    open: 'Display settings',
    title: 'Display',
    description: 'Changes apply to this screen only.',
    colorScheme: 'Brightness',
    scheme: {
      system: 'Automatic',
      light: 'Light',
      dark: 'Dark',
    },
    style: 'Style',
    preset: {
      default: 'Classic',
      'glass-dark': 'Glass',
    },
    homeStyle: "Use the home's style",
    effects: 'See-through effects',
    effectsHint: 'Turn off if this screen feels slow.',
    language: 'Language',
  },
  language: {
    en: 'English',
    es: 'Español',
  },
  widget: {
    unavailable: 'Not responding',
    pending: 'Working…',
    turnOn: 'Turn on {{name}}',
    turnOff: 'Turn off {{name}}',
    commandFailed: "{{name}} didn't respond. Please try again.",
    unknownDevice: 'Unknown device',
  },
  commandError: {
    unsupported: "{{name}} can't do that.",
    offline: "{{name}} isn't responding. Check that it has power.",
    notAllowed: "This screen isn't allowed to control {{name}}.",
    confirmationRequired: '{{name}} needs you to confirm first. Please try again.',
    device: 'Something went wrong with {{name}}. Please try again.',
    notConnected: "Can't reach your home right now. Please try again in a moment.",
  },
  dashboard: {
    loadFailedTitle: "Can't show your home yet",
    loadFailedBody: 'This screen will keep trying by itself.',
    noPagesTitle: 'Nothing to show yet',
    noPagesBody: 'Pages added for your home will show up here.',
    emptyPageTitle: 'This page is empty',
    emptyPageBody: 'Tiles added to this page will show up here.',
  },
  doorbell: {
    title: 'Someone is at the door',
    open: 'Open the door',
    dismiss: 'Close',
    autoClose: 'Closes by itself in {{seconds}} s',
  },
  screensaver: {
    label: 'Screen saver',
    hint: 'Touch the screen to wake it up',
  },
  light: {
    brightness: 'Brightness',
    whiteTone: 'White tone',
    warm: 'Warm',
    cool: 'Cool',
  },
  fan: {
    speed: 'Speed',
    speedStep: 'Speed {{speed}}',
    direction: 'Direction',
    forward: 'Normal',
    reverse: 'Reverse',
  },
  cover: {
    open: 'Open',
    close: 'Close',
    stop: 'Stop',
    opening: 'Opening…',
    closing: 'Closing…',
    position: 'How open',
    openPercent: '{{value}}% open',
    isOpen: 'Open',
    isClosed: 'Closed',
    confirmOpenTitle: 'Open {{name}}?',
    confirmOpenBody: 'Make sure it is safe before opening.',
  },
  door: {
    open: 'Open',
    opening: 'Opening…',
    confirmTitle: 'Open {{name}}?',
    confirmBody: 'Make sure you know who is at the door.',
    confirm: 'Open',
    opened: '{{name}} is opening',
  },
  camera: {
    notAvailable: "Live video isn't available yet.",
  },
  clock: {
    weatherSoon: 'Weather is coming soon.',
  },
  unknownWidget: {
    title: "This tile can't be shown",
    body: 'It needs a newer version of the app.',
  },
  demo: {
    badge: 'Sample home',
    notice: 'Changes here are not real.',
    pages: {
      home: 'Home',
      livingRoom: 'Living room',
      outside: 'Outside',
    },
    devices: {
      livingRoomLight: 'Living room light',
      ceilingFan: 'Ceiling fan',
      fanLight: 'Fan light',
      kitchenLight: 'Kitchen light',
      hallwayLight: 'Hallway light',
      porchLight: 'Porch light',
      bedroomBlind: 'Bedroom blind',
      patioAwning: 'Patio awning',
      garageDoor: 'Garage door',
      frontDoor: 'Front door',
      frontDoorCamera: 'Front door camera',
      gardenCamera: 'Garden camera',
      clockPlace: 'Home',
    },
  },
}

export type Translation = typeof en
