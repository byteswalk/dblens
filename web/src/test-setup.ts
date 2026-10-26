// 测试默认用中文界面（断言多按中文文字写）；需要英文的测试自己调用 i18n.changeLanguage('en')。
import i18n from './shared/i18n'

await i18n.changeLanguage('zh-CN')
