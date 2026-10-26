// PG 系（PostgreSQL、KingbaseES、openGauss、GaussDB、HighGo）的语言服务，单独成块懒加载。

import { PostgreSQL } from 'dt-sql-parser'

import { adapt } from './dtsql'

export const postgres = adapt(new PostgreSQL())
