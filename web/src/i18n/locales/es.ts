import type { Translation } from '@/i18n/locales/en'

/**
 * Textos en español. Mismas claves que `en.ts` (el tipo lo garantiza).
 * Lenguaje sencillo: nunca "servidor", "IP", "controlador", "token"...
 */
export const es: Translation = {
  app: {
    name: 'VESTA',
  },
  common: {
    cancel: 'Cancelar',
    close: 'Cerrar',
    notifications: 'Notificaciones',
    on: 'Encendido',
    off: 'Apagado',
    percent: '{{value}} %',
  },
  nav: {
    label: 'Habitaciones y páginas',
  },
  connection: {
    connecting: 'Conectando con tu casa…',
    lost: 'Ahora mismo no se puede conectar con tu casa.',
    retrying: 'Reintentando…',
    retryingIn: 'Reintentando en {{seconds}} s',
    retryNow: 'Reintentar ahora',
    connected: 'Conectado con tu casa',
    waitingTitle: 'Esperando a tu casa',
    waitingBody: 'Esta pantalla mostrará tu casa en cuanto se conecte.',
  },
  display: {
    open: 'Ajustes de pantalla',
    title: 'Pantalla',
    description: 'Los cambios solo afectan a esta pantalla.',
    colorScheme: 'Luminosidad',
    scheme: {
      system: 'Automático',
      light: 'Claro',
      dark: 'Oscuro',
    },
    style: 'Estilo',
    preset: {
      default: 'Clásico',
      'glass-dark': 'Cristal',
    },
    homeStyle: 'Usar el estilo de la casa',
    effects: 'Efectos de transparencia',
    effectsHint: 'Desactívalos si la pantalla va lenta.',
    language: 'Idioma',
  },
  language: {
    en: 'English',
    es: 'Español',
  },
  widget: {
    unavailable: 'No responde',
    pending: 'En curso…',
    turnOn: 'Encender {{name}}',
    turnOff: 'Apagar {{name}}',
    commandFailed: '{{name}} no ha respondido. Inténtalo de nuevo.',
    unknownDevice: 'Dispositivo desconocido',
  },
  commandError: {
    unsupported: '{{name}} no puede hacer eso.',
    offline: '{{name}} no responde. Comprueba que tiene corriente.',
    notAllowed: 'Esta pantalla no tiene permiso para controlar {{name}}.',
    confirmationRequired: '{{name}} necesita que confirmes primero. Inténtalo de nuevo.',
    device: 'Algo ha fallado con {{name}}. Inténtalo de nuevo.',
    notConnected: 'Ahora mismo no se puede conectar con tu casa. Inténtalo en un momento.',
  },
  dashboard: {
    loadFailedTitle: 'Aún no se puede mostrar tu casa',
    loadFailedBody: 'Esta pantalla lo seguirá intentando sola.',
    noPagesTitle: 'Todavía no hay nada que mostrar',
    noPagesBody: 'Las páginas de tu casa aparecerán aquí.',
    emptyPageTitle: 'Esta página está vacía',
    emptyPageBody: 'Los recuadros que se añadan a esta página aparecerán aquí.',
  },
  doorbell: {
    title: 'Hay alguien en la puerta',
    open: 'Abrir la puerta',
    dismiss: 'Cerrar',
    autoClose: 'Se cierra sola en {{seconds}} s',
  },
  screensaver: {
    label: 'Salvapantallas',
    hint: 'Toca la pantalla para activarla',
  },
  light: {
    brightness: 'Brillo',
    whiteTone: 'Tono de blanco',
    warm: 'Cálido',
    cool: 'Frío',
  },
  fan: {
    speed: 'Velocidad',
    speedStep: 'Velocidad {{speed}}',
    direction: 'Sentido',
    forward: 'Normal',
    reverse: 'Inverso',
  },
  cover: {
    open: 'Abrir',
    close: 'Cerrar',
    stop: 'Parar',
    opening: 'Abriendo…',
    closing: 'Cerrando…',
    position: 'Apertura',
    openPercent: 'Apertura {{value}} %',
    isOpen: 'Abierto',
    isClosed: 'Cerrado',
    confirmOpenTitle: '¿Abrir {{name}}?',
    confirmOpenBody: 'Asegúrate de que es seguro antes de abrir.',
  },
  door: {
    open: 'Abrir',
    opening: 'Abriendo…',
    confirmTitle: '¿Abrir {{name}}?',
    confirmBody: 'Asegúrate de saber quién está en la puerta.',
    confirm: 'Abrir',
    opened: 'Abriendo {{name}}',
  },
  camera: {
    notAvailable: 'El vídeo en directo aún no está disponible.',
  },
  clock: {
    weatherSoon: 'El tiempo llegará pronto.',
  },
  unknownWidget: {
    title: 'Este recuadro no se puede mostrar',
    body: 'Necesita una versión más nueva de la app.',
  },
  demo: {
    badge: 'Casa de ejemplo',
    notice: 'Los cambios aquí no son reales.',
    pages: {
      home: 'Inicio',
      livingRoom: 'Salón',
      outside: 'Exterior',
    },
    devices: {
      livingRoomLight: 'Luz del salón',
      ceilingFan: 'Ventilador de techo',
      fanLight: 'Luz del ventilador',
      kitchenLight: 'Luz de la cocina',
      hallwayLight: 'Luz del pasillo',
      porchLight: 'Luz del porche',
      bedroomBlind: 'Persiana del dormitorio',
      patioAwning: 'Toldo de la terraza',
      garageDoor: 'Puerta del garaje',
      frontDoor: 'Puerta de la calle',
      frontDoorCamera: 'Cámara de la entrada',
      gardenCamera: 'Cámara del jardín',
      clockPlace: 'Casa',
    },
  },
}
