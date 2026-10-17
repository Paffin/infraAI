// Отказы сервера, которые видит человек: размер, срок, нагрузка и значения настроек. Сервер присылает стабильный `code` и английское предложение для скриптов;
// страница говорит своими словами (docs/API.md, «Ошибки»). Ключи те же, что в en.errors.ts.
export default {
  'error.fieldTooLong': 'Поле «{field}» слишком длинное: не больше {max} знаков, сейчас {actual}',
  'error.invalidName': 'Название: от 1 до {max} знаков, сейчас {actual}',
  'error.opsTooMany': 'За один запрос — не больше {max} правок, а их {actual}',
  'error.requestTimeout': 'Сервер не успел ответить, запрос остановлен. Повторите или упростите схему',
  'error.payloadTooLarge': 'Запрос больше, чем принимает сервер',
  'error.busy': 'Сервер занят другими расчётами. Повторите через {seconds} с',
  'error.busy.soon': 'Сервер занят другими расчётами. Повторите чуть позже',
  'error.busy.user': 'У вас уже идёт столько расчётов, сколько сервер ведёт для одного человека ({limit}). Подождите, пока один закончится: около {seconds} с',
  'error.field.name': 'Имя',
  'error.field.description': 'Описание',
  'error.field.message': 'Сообщение',
  'error.field.reason': 'Обоснование',
  'error.field.fingerprint': 'Отпечаток находки',
  'error.field.findingId': 'Идентификатор находки',

  // значение настройки, которое сервер не принял (`details.reasonCode` ответа `invalid_setting`); below_min, above_max, not_an_option и wrong_type — тексты формы (admin.settings.err.*, ключи формы)
  'error.setting.entry': '«{value}»: {reason}',
  'error.setting.not_single_line': 'Нужна одна строка: без переводов строки и управляющих символов',
  'error.setting.line_too_long': 'Не длиннее {max} знаков',
  'error.setting.not_http': 'Нужен адрес, который начинается с http:// или https://',
  'error.setting.address_chars': 'В адресе не должно быть пробелов, обратной косой черты и управляющих символов',
  'error.setting.address_too_long': 'Адрес не длиннее {max} знаков',
  'error.setting.bad_address': 'Это не адрес: проверьте, как он записан',
  'error.setting.no_host': 'В адресе нет имени хоста',
  'error.setting.has_credentials': 'В адресе не должно быть имени пользователя и пароля',
  'error.setting.has_query': 'В адресе не должно быть строки запроса (?) и якоря (#)',
  'error.setting.not_origin': 'Нужен источник вида схема://хост[:порт] — без пути, строки запроса и якоря',
  'error.setting.empty': 'Значение не может быть пустым',
  'error.setting.unknown_setting': 'Такой настройки нет',
  'error.setting.cannot_store': 'База данных не принимает это значение',
} as Record<string, string>
