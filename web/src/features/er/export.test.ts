import { describe, expect, it } from 'vitest'

import { keepDeclarations, splitStyle } from './export'

describe('导出 SVG 时精简样式', () => {
  it('拆声明时不拆引号和括号中的分号', () => {
    expect(splitStyle('color: red; background: url("a;b") no-repeat; font-family: "x;y", serif; --v: 1;')).toEqual([
      ['color', 'red'],
      ['background', 'url("a;b") no-repeat'],
      ['font-family', '"x;y", serif'],
      ['--v', '1'],
    ])
  })

  it('去掉 CSS 变量、和上级相同的继承属性、和默认值相同的不继承属性', () => {
    const defaults = new Map([
      ['display', 'block'],
      ['color', 'rgb(0, 0, 0)'],
      ['margin', '0px'],
    ])
    const parent = new Map([
      ['color', 'rgb(255, 255, 255)'],
      ['display', 'flex'],
    ])
    const own: [string, string][] = [
      ['--ant-color', 'red'],
      ['display', 'block'],
      ['margin', '4px'],
      ['color', 'rgb(255, 255, 255)'],
      ['font-size', '13.9px'],
    ]
    expect(keepDeclarations(own, parent, defaults)).toEqual([
      ['margin', '4px'],
      ['font-size', '13.9px'],
    ])
  })

  it('继承属性和默认值相同、但上级不同时保留，避免继承到上级的值', () => {
    const defaults = new Map([['color', 'rgb(0, 0, 0)']])
    const parent = new Map([['color', 'rgb(255, 255, 255)']])
    expect(keepDeclarations([['color', 'rgb(0, 0, 0)']], parent, defaults)).toEqual([['color', 'rgb(0, 0, 0)']])
    // 没有上级时按默认值判断
    expect(keepDeclarations([['color', 'rgb(0, 0, 0)']], null, defaults)).toEqual([])
    // -webkit- 前缀的继承属性同样按上级判断
    expect(keepDeclarations([['-webkit-font-smoothing', 'auto']], new Map([['-webkit-font-smoothing', 'auto']]), new Map())).toEqual([])
  })

  it('不继承的颜色属性按当前文字颜色比默认值；color 本身不替换', () => {
    const defaults = new Map([
      ['color', 'rgb(0, 0, 0)'],
      ['border-color', 'rgb(0, 0, 0)'],
      ['outline', 'rgb(0, 0, 0) none 0px'],
      ['stop-color', 'rgb(0, 0, 0)'],
    ])
    const white = 'rgb(255, 255, 255)'
    const own: [string, string][] = [
      ['color', white],
      ['border-color', white],
      ['outline', `${white} none 0px`],
      ['stop-color', white],
    ]
    // 没有上级：color 要留下；边框、轮廓颜色等于文字颜色，是默认值；stop-color 默认是黑色，不随文字颜色变
    expect(keepDeclarations(own, null, defaults)).toEqual([
      ['color', white],
      ['stop-color', white],
    ])
  })

  it('交互相关的属性和逻辑属性不要', () => {
    const own: [string, string][] = [
      ['cursor', 'pointer'],
      ['app-region', 'no-drag'],
      ['block-size', '30px'],
      ['border-inline', '1px solid red'],
      ['height', '30px'],
    ]
    expect(keepDeclarations(own, null, new Map())).toEqual([['height', '30px']])
  })
})
